//===- SplitStackPass.cpp - Hopter-style split stack instrumentation ------===//
//
// An out-of-tree LLVM pass that inserts a per-function stack-check prologue,
// implementing a Hopter-style segmented stack scheme without any compiler
// modification.  Designed for aarch64-linux-gnu (demo) but the IR-level
// instrumentation itself is target-agnostic; only the runtime cares about
// the SP / TLS conventions of the target.
//
// Build:
//     cmake -S . -B build && cmake --build build
//
// Use:
//     clang -O1 -fpass-plugin=./build/SplitStackPass.so foo.c ...
//
// What this pass does, for every defined function F:
//
//     entry:
//        %sp    = call i64 @llvm.read_register.i64(metadata !"sp")
//        %bound = load i64, i64* @__hopter_stklet_bound      ; thread-local
//        %need  = sub i64 %sp, <ESTIMATED_FRAME_SIZE>
//        %ok    = icmp uge i64 %need, %bound
//        br i1 %ok, label %ss.cont, label %ss.morestack
//     ss.morestack:
//        call void @__morestack(i64 <ESTIMATED_FRAME_SIZE>)
//        br label %ss.cont
//     ss.cont:
//        ; ... original entry block ...
//
// Frame size is estimated from the IR (sum of static allocas + a
// conservative padding for callee-saved/spill).  Over-estimation only
// causes earlier stacklet allocation, never undefined behaviour.
//
//===----------------------------------------------------------------------===//

#include "llvm/IR/Function.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/InlineAsm.h"
#include "llvm/IR/Instructions.h"
#include "llvm/IR/Intrinsics.h"
#include "llvm/IR/LLVMContext.h"
#include "llvm/IR/MDBuilder.h"
#include "llvm/IR/Metadata.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/PassManager.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Passes/PassPlugin.h"
#include "llvm/Support/CommandLine.h"
#include "llvm/Support/Debug.h"
#include "llvm/Support/raw_ostream.h"
#include <cstdlib>

using namespace llvm;

#define DEBUG_TYPE "split-stack"

namespace {

// Conservative padding covering callee-saved registers, register spills,
// stack canary, alignment, and other backend-invisible overhead.
// Also serves as the rolling safety window: A's check guarantees
// kFramePadding bytes below A's post-prologue SP, which is enough for
// B's prologue writes to complete safely before B runs its own check.
static constexpr uint64_t kFramePadding = 128;

static cl::opt<bool> ClVerbose(
    "split-stack-verbose",
    cl::desc("Print per-function instrumentation decisions"),
    cl::init(false), cl::Hidden);

static bool verboseEnabled() {
  if (ClVerbose) return true;
  const char *e = std::getenv("SPLIT_STACK_VERBOSE");
  return e && *e && *e != '0';
}

static cl::opt<unsigned> ClExtraPad(
    "split-stack-extra-pad",
    cl::desc("Extra bytes added to every frame estimate"),
    cl::init(0), cl::Hidden);

//===----------------------------------------------------------------------===//
// Instrumentation
//===----------------------------------------------------------------------===//

static bool shouldSkip(const Function &F) {
  if (F.isDeclaration()) return true;
  if (F.hasFnAttribute("no-split-stack")) return true;
  if (F.hasFnAttribute(Attribute::Naked)) return true;
  // Avoid recursing into our own runtime helpers.
  StringRef N = F.getName();
  if (N.starts_with("__morestack")) return true;
  if (N.starts_with("__split_stack")) return true;
  if (N == "__hopter_stklet_bound") return true;
  return false;
}

static GlobalVariable *getOrCreateStackletBound(Module &M) {
  const char *kName = "__hopter_stklet_bound";
  if (auto *GV = M.getNamedGlobal(kName)) return GV;

  Type *I64 = Type::getInt64Ty(M.getContext());
  auto *GV = new GlobalVariable(
      M, I64, /*isConstant=*/false, GlobalValue::ExternalLinkage,
      /*Initializer=*/nullptr, kName, /*InsertBefore=*/nullptr,
      GlobalValue::GeneralDynamicTLSModel);
  GV->setAlignment(Align(8));
  return GV;
}

static FunctionCallee getMorestack(Module &M) {
  LLVMContext &C = M.getContext();
  FunctionType *FT =
      FunctionType::get(Type::getVoidTy(C), {Type::getInt64Ty(C)},
                        /*isVarArg=*/false);
  return M.getOrInsertFunction("__morestack", FT);
}

static bool instrumentFunction(Function &F) {
  if (shouldSkip(F)) return false;

  Module *M = F.getParent();
  LLVMContext &C = M->getContext();

  // The backend inserts the prologue (sub sp, sp, #N; stp ...) before any IR
  // instruction.  So reading SP at IR entry gives the post-prologue SP, which
  // already accounts for all static allocas and callee-save slots.  We only
  // need to reserve kFramePadding bytes below that as a rolling safety window
  // for the next callee's prologue writes.
  uint64_t frame = ((kFramePadding + ClExtraPad + 15) & ~uint64_t(15));
  if (verboseEnabled()) {
    errs() << "[split-stack] " << F.getName()
           << " frame pad = " << frame << " bytes\n";
  }

  // ---------------------------------------------------------------
  // Split entry block:
  //   old_entry → ss.cont (everything that was in entry stays here).
  //   We then prepend a new check block + morestack block.
  // ---------------------------------------------------------------
  BasicBlock &OldEntry = F.getEntryBlock();
  BasicBlock *Cont = OldEntry.splitBasicBlock(OldEntry.getFirstInsertionPt(),
                                              "ss.cont");
  // After splitBasicBlock, OldEntry contains a single unconditional br to Cont.
  // We replace that terminator with our check.

  Instruction *OldTerm = OldEntry.getTerminator();
  IRBuilder<> B(OldTerm);

  // %sp = call i64 @llvm.read_register.i64(metadata !"sp")
  Function *ReadReg = Intrinsic::getDeclaration(M, Intrinsic::read_register,
                                                {Type::getInt64Ty(C)});
  MDNode *SPName = MDNode::get(C, MDString::get(C, "sp"));
  Value *SP = B.CreateCall(ReadReg, {MetadataAsValue::get(C, SPName)},
                           "ss.sp");

  // %bound = load i64, i64* @__hopter_stklet_bound
  GlobalVariable *Bound = getOrCreateStackletBound(*M);
  LoadInst *BoundVal = B.CreateLoad(Type::getInt64Ty(C), Bound, "ss.bound");
  BoundVal->setAlignment(Align(8));

  // %need = sub i64 %sp, <frame>
  Value *Need =
      B.CreateSub(SP, ConstantInt::get(Type::getInt64Ty(C), frame), "ss.need");

  // %ok = icmp uge i64 %need, %bound
  Value *Ok = B.CreateICmpUGE(Need, BoundVal, "ss.ok");

  // Create morestack block and replace old terminator.
  BasicBlock *More = BasicBlock::Create(C, "ss.morestack", &F, Cont);
  B.CreateCondBr(Ok, Cont, More,
                 MDBuilder(C).createBranchWeights(/*True=*/1024, /*False=*/1));
  OldTerm->eraseFromParent();

  // ss.morestack: tail call __morestack(frame); br ss.cont
  IRBuilder<> MB(More);
  CallInst *Call =
      MB.CreateCall(getMorestack(*M),
                    {ConstantInt::get(Type::getInt64Ty(C), frame)});
  Call->setDoesNotThrow();
  MB.CreateBr(Cont);

  // Annotate the function so later passes (and humans reading IR) know
  // we instrumented it.
  F.addFnAttr("split-stack-instrumented");
  F.addFnAttr("split-stack-frame-estimate", std::to_string(frame));

  // ---------------------------------------------------------------
  // Instrument dynamic allocas (VLAs).
  //
  // Dynamic allocas lower to a runtime `sub sp, sp, n`.  After that
  // instruction executes, SP already reflects the allocation.  We
  // insert the check immediately after the alloca using the actual
  // post-allocation SP, requiring SP - kFramePadding >= bound.
  //
  // This is identical in form to the entry check: both read the
  // current (already-adjusted) SP and subtract the same constant
  // padding.  No need to know the alloca size at compile time.
  //
  // Inserted shape (per dynamic alloca):
  //
  //   PreBB:                        ; instructions before the alloca
  //     br ss.dyn.alloca
  //   ss.dyn.alloca:
  //     %buf = alloca ...           ; SP adjusted here by backend
  //     %ss.dyn.sp    = read_sp    ; post-allocation SP
  //     %ss.dyn.bound = load __hopter_stklet_bound
  //     %ss.dyn.need  = sp - kFramePadding
  //     %ss.dyn.ok    = icmp uge need, bound
  //     br ok → ss.dyn.cont, ss.dyn.morestack [weights 1024:1]
  //   ss.dyn.morestack:
  //     call __morestack(kFramePadding)
  //     br ss.dyn.cont
  //   ss.dyn.cont:
  //     ...                         ; rest of original block
  // ---------------------------------------------------------------
  {
    std::vector<AllocaInst *> DynAllocas;
    for (BasicBlock &BB : F)
      for (Instruction &I : BB)
        if (auto *AI = dyn_cast<AllocaInst>(&I))
          if (!AI->isStaticAlloca())
            DynAllocas.push_back(AI);

    for (AllocaInst *AI : DynAllocas) {
      // Split so that AI is the first instruction of AllocaBB.
      BasicBlock *AllocaBB =
          AI->getParent()->splitBasicBlock(AI, "ss.dyn.alloca");
      // Split again after AI: RestBB starts with the instruction after AI.
      BasicBlock *RestBB = AllocaBB->splitBasicBlock(
          &*std::next(BasicBlock::iterator(AI)), "ss.dyn.cont");
      // AllocaBB now contains: { AI, br RestBB }

      // Replace AllocaBB's terminator with the check.
      Instruction *AllocaTerm = AllocaBB->getTerminator();
      IRBuilder<> DB(AllocaTerm);

      // Read post-alloca SP.
      Value *DSP = DB.CreateCall(ReadReg,
                                 {MetadataAsValue::get(C, SPName)},
                                 "ss.dyn.sp");
      LoadInst *DBound = DB.CreateLoad(Type::getInt64Ty(C), Bound,
                                       "ss.dyn.bound");
      DBound->setAlignment(Align(8));

      Value *DNeed = DB.CreateSub(
          DSP, ConstantInt::get(Type::getInt64Ty(C), frame), "ss.dyn.need");
      Value *DOk = DB.CreateICmpUGE(DNeed, DBound, "ss.dyn.ok");

      BasicBlock *DMore =
          BasicBlock::Create(C, "ss.dyn.morestack", &F, RestBB);
      DB.CreateCondBr(DOk, RestBB, DMore,
                      MDBuilder(C).createBranchWeights(1024, 1));
      AllocaTerm->eraseFromParent();

      // ss.dyn.morestack: call __morestack(frame); br RestBB.
      IRBuilder<> DMB(DMore);
      CallInst *DCall = DMB.CreateCall(
          getMorestack(*M),
          {ConstantInt::get(Type::getInt64Ty(C), frame)});
      DCall->setDoesNotThrow();
      DMB.CreateBr(RestBB);

      if (verboseEnabled())
        errs() << "[split-stack]   + dyn-alloca check after "
               << AI->getName() << " in " << AllocaBB->getName() << "\n";
    }
  }

  return true;
}

//===----------------------------------------------------------------------===//
// Pass plumbing (new PassManager)
//===----------------------------------------------------------------------===//

struct SplitStackPass : PassInfoMixin<SplitStackPass> {
  PreservedAnalyses run(Module &M, ModuleAnalysisManager &) {
    bool changed = false;
    for (Function &F : M) {
      changed |= instrumentFunction(F);
    }
    return changed ? PreservedAnalyses::none() : PreservedAnalyses::all();
  }

  static bool isRequired() { return true; }
};

} // namespace

extern "C" LLVM_ATTRIBUTE_WEAK ::llvm::PassPluginLibraryInfo
llvmGetPassPluginInfo() {
  return {LLVM_PLUGIN_API_VERSION, "SplitStackPass",
          LLVM_VERSION_STRING,
          [](PassBuilder &PB) {
            // Run very late so the IR we see already has all optimizations
            // applied; this gives us a more accurate alloca picture.
            PB.registerOptimizerLastEPCallback(
                [](ModulePassManager &MPM, OptimizationLevel) {
                  MPM.addPass(SplitStackPass());
                });
            // Also expose under -passes=split-stack for opt-driven testing.
            PB.registerPipelineParsingCallback(
                [](StringRef Name, ModulePassManager &MPM,
                   ArrayRef<PassBuilder::PipelineElement>) {
                  if (Name == "split-stack") {
                    MPM.addPass(SplitStackPass());
                    return true;
                  }
                  return false;
                });
          }};
}
