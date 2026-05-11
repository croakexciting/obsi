cmake_minimum_required(VERSION 3.16)
project(SplitStackPass LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# Default to LLVM 18 if not overridden
if(NOT DEFINED LLVM_DIR)
    set(LLVM_DIR "/usr/lib/llvm-18/lib/cmake/llvm")
endif()

find_package(LLVM REQUIRED CONFIG)
message(STATUS "Found LLVM ${LLVM_PACKAGE_VERSION} at ${LLVM_DIR}")

include_directories(SYSTEM ${LLVM_INCLUDE_DIRS})
add_definitions(${LLVM_DEFINITIONS})

add_library(SplitStackPass MODULE pass/SplitStackPass.cpp)

# LLVM is built without RTTI by default; the pass plugin must match.
target_compile_options(SplitStackPass PRIVATE -fno-rtti -Wall -Wextra)

# Pass plugins do not link against libLLVM (symbols come from the host).
set_target_properties(SplitStackPass PROPERTIES
    PREFIX ""           # produce SplitStackPass.so (no lib prefix) for clarity
    OUTPUT_NAME "SplitStackPass"
)
