use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

trait RunPendedOp {
    fn run_pended_op(&mut self);
}

trait AllowPendOp<'a> {
    type FullAccessor: RunPendedOp + 'a;
    type PendOnlyAccessor: 'a;

    fn full_access(&'a self) -> Self::FullAccessor;
    fn pend_only_access(&'a self) -> Self::PendOnlyAccessor;
}

enum Access<'a, T>
where
    T: AllowPendOp<'a>,
{
    Full {
        full_access: <T as AllowPendOp<'a>>::FullAccessor,
    },
    PendOnly {
        pend_access: <T as AllowPendOp<'a>>::PendOnlyAccessor,
    },
}

struct SoftLock<T>
where
    for<'a> T: AllowPendOp<'a>,
{
    content: T,
    pending: AtomicBool,
    locked: AtomicBool,
}

impl<T> SoftLock<T>
where
    for<'a> T: AllowPendOp<'a>,
{
    fn new(content: T) -> Self {
        Self {
            content,
            pending: AtomicBool::new(false),
            locked: AtomicBool::new(false),
        }
    }

    fn with_access<R>(&self, op: impl for<'a> FnOnce(Access<'a, T>) -> R) -> R {
        let guard = AccessGuard::guard(self);
        let access = if guard.lock_held {
            Access::Full {
                full_access: self.content.full_access(),
            }
        } else {
            Access::PendOnly {
                pend_access: self.content.pend_only_access(),
            }
        };

        let output = op(access);
        drop(guard);
        output
    }
}

struct AccessGuard<'a, T>
where
    for<'b> T: AllowPendOp<'b>,
{
    lock_held: bool,
    soft_lock: &'a SoftLock<T>,
}

impl<'a, T> AccessGuard<'a, T>
where
    for<'b> T: AllowPendOp<'b>,
{
    fn guard(soft_lock: &'a SoftLock<T>) -> Self {
        Self {
            lock_held: soft_lock
                .locked
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            soft_lock,
        }
    }
}

impl<'a, T> Drop for AccessGuard<'a, T>
where
    for<'b> T: AllowPendOp<'b>,
{
    fn drop(&mut self) {
        if self.lock_held {
            loop {
                let prev_pending = self.soft_lock.pending.swap(false, Ordering::SeqCst);
                if prev_pending {
                    let mut full_access = self.soft_lock.content.full_access();
                    full_access.run_pended_op();
                }

                self.soft_lock.locked.store(false, Ordering::SeqCst);

                if !self.soft_lock.pending.load(Ordering::SeqCst) {
                    break;
                }

                while self
                    .soft_lock
                    .locked
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    std::hint::spin_loop();
                }
            }
        } else {
            self.soft_lock.pending.store(true, Ordering::SeqCst);
        }
    }
}

struct DemoInner {
    pending_submit: AtomicUsize,
    queue: Mutex<VecDeque<u64>>,
    next_id: AtomicU64,
    processed: AtomicUsize,
    drained_items: AtomicUsize,
    drained_batches: AtomicUsize,
    direct_items: AtomicUsize,
    full_entries: AtomicUsize,
    pend_entries: AtomicUsize,
}

impl DemoInner {
    fn new() -> Self {
        Self {
            pending_submit: AtomicUsize::new(0),
            queue: Mutex::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
            processed: AtomicUsize::new(0),
            drained_items: AtomicUsize::new(0),
            drained_batches: AtomicUsize::new(0),
            direct_items: AtomicUsize::new(0),
            full_entries: AtomicUsize::new(0),
            pend_entries: AtomicUsize::new(0),
        }
    }
}

struct InnerFullAccessor<'a> {
    inner: &'a DemoInner,
}

struct InnerPendAccessor<'a> {
    inner: &'a DemoInner,
}

impl<'a> InnerFullAccessor<'a> {
    fn submit_direct(&mut self) {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let mut queue = self.inner.queue.lock().expect("queue mutex poisoned");
        queue.push_back(id);
        self.inner.direct_items.fetch_add(1, Ordering::SeqCst);
    }

    fn process_some(&mut self, mut budget: usize) -> usize {
        let mut processed_now = 0;
        let mut queue = self.inner.queue.lock().expect("queue mutex poisoned");
        while budget > 0 {
            if queue.pop_front().is_some() {
                self.inner.processed.fetch_add(1, Ordering::SeqCst);
                processed_now += 1;
            } else {
                break;
            }
            budget -= 1;
        }
        processed_now
    }

    fn queue_len(&self) -> usize {
        let queue = self.inner.queue.lock().expect("queue mutex poisoned");
        queue.len()
    }
}

impl<'a> RunPendedOp for InnerFullAccessor<'a> {
    fn run_pended_op(&mut self) {
        let pending = self.inner.pending_submit.swap(0, Ordering::SeqCst);
        if pending == 0 {
            return;
        }

        self.inner.drained_batches.fetch_add(1, Ordering::SeqCst);
        self.inner.drained_items.fetch_add(pending, Ordering::SeqCst);

        let mut queue = self.inner.queue.lock().expect("queue mutex poisoned");
        for _ in 0..pending {
            let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
            queue.push_back(id);
        }
    }
}

impl<'a> InnerPendAccessor<'a> {
    fn pend_submit(&self) {
        self.inner.pending_submit.fetch_add(1, Ordering::SeqCst);
    }
}

impl<'a> AllowPendOp<'a> for DemoInner {
    type FullAccessor = InnerFullAccessor<'a>;
    type PendOnlyAccessor = InnerPendAccessor<'a>;

    fn full_access(&'a self) -> Self::FullAccessor {
        InnerFullAccessor { inner: self }
    }

    fn pend_only_access(&'a self) -> Self::PendOnlyAccessor {
        InnerPendAccessor { inner: self }
    }
}

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    producers: usize,
    ops_per_producer: usize,
    producer_pause_every: usize,
    consumer_budget: usize,
    consumer_hold_spin: usize,
}

#[derive(Debug)]
struct SoftResult {
    elapsed: Duration,
    throughput_mops: f64,
    full_entries: usize,
    pend_entries: usize,
    drained_items: usize,
    drained_batches: usize,
    avg_batch: f64,
}

#[derive(Debug)]
struct MutexResult {
    elapsed: Duration,
    throughput_mops: f64,
    lock_acquires_producer: usize,
    lock_acquires_consumer: usize,
}

struct MutexInner {
    queue: Mutex<VecDeque<u64>>,
    next_id: AtomicU64,
    processed: AtomicUsize,
    producer_lock_count: AtomicUsize,
    consumer_lock_count: AtomicUsize,
}

impl MutexInner {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            next_id: AtomicU64::new(1),
            processed: AtomicUsize::new(0),
            producer_lock_count: AtomicUsize::new(0),
            consumer_lock_count: AtomicUsize::new(0),
        }
    }
}

fn spin_n(iters: usize) {
    for _ in 0..iters {
        std::hint::spin_loop();
    }
}

fn run_softlock_bench(s: Scenario) -> SoftResult {
    let total_ops = s.producers * s.ops_per_producer;
    let lock = Arc::new(SoftLock::new(DemoInner::new()));
    let producers_done = Arc::new(AtomicBool::new(false));

    let consumer_lock = Arc::clone(&lock);
    let consumer_done = Arc::clone(&producers_done);
    let consumer = thread::spawn(move || {
        while consumer_lock.with_access(|access| match access {
            Access::Full { mut full_access } => {
                let pending_before = full_access.inner.pending_submit.load(Ordering::SeqCst);
                full_access.run_pended_op();
                let processed_now = full_access.process_some(s.consumer_budget);
                if pending_before > 0 || processed_now > 0 {
                    spin_n(s.consumer_hold_spin);
                }

                let processed = full_access.inner.processed.load(Ordering::SeqCst);
                if processed >= total_ops {
                    return false;
                }

                let producers_finished = consumer_done.load(Ordering::SeqCst);
                let pending = full_access.inner.pending_submit.load(Ordering::SeqCst);
                let queue_len = full_access.queue_len();
                !(producers_finished && pending == 0 && queue_len == 0)
            }
            Access::PendOnly { .. } => true,
        }) {
            thread::yield_now();
        }
    });

    let start = Instant::now();
    let mut handles = Vec::with_capacity(s.producers);
    for p in 0..s.producers {
        let lock = Arc::clone(&lock);
        handles.push(thread::spawn(move || {
            for i in 0..s.ops_per_producer {
                lock.with_access(|access| match access {
                    Access::Full { mut full_access } => {
                        full_access.inner.full_entries.fetch_add(1, Ordering::SeqCst);
                        full_access.submit_direct();
                    }
                    Access::PendOnly { pend_access } => {
                        pend_access.inner.pend_entries.fetch_add(1, Ordering::SeqCst);
                        pend_access.pend_submit();
                    }
                });

                if s.producer_pause_every != 0 && (i + p) % s.producer_pause_every == 0 {
                    thread::yield_now();
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("producer panicked");
    }
    producers_done.store(true, Ordering::SeqCst);
    consumer.join().expect("consumer panicked");

    let elapsed = start.elapsed();

    let (processed, pending, queue_left, full_entries, pend_entries, drained_items, drained_batches) =
        lock.with_access(|access| match access {
            Access::Full { full_access } => (
                full_access.inner.processed.load(Ordering::SeqCst),
                full_access.inner.pending_submit.load(Ordering::SeqCst),
                full_access.queue_len(),
                full_access.inner.full_entries.load(Ordering::SeqCst),
                full_access.inner.pend_entries.load(Ordering::SeqCst),
                full_access.inner.drained_items.load(Ordering::SeqCst),
                full_access.inner.drained_batches.load(Ordering::SeqCst),
            ),
            Access::PendOnly { .. } => unreachable!("all workers joined"),
        });

    assert_eq!(processed, total_ops, "softlock processed mismatch");
    assert_eq!(pending, 0, "softlock pending should be empty");
    assert_eq!(queue_left, 0, "softlock queue should be empty");

    let avg_batch = if drained_batches == 0 {
        0.0
    } else {
        drained_items as f64 / drained_batches as f64
    };

    SoftResult {
        elapsed,
        throughput_mops: total_ops as f64 / elapsed.as_secs_f64() / 1_000_000.0,
        full_entries,
        pend_entries,
        drained_items,
        drained_batches,
        avg_batch,
    }
}

fn run_mutex_bench(s: Scenario) -> MutexResult {
    let total_ops = s.producers * s.ops_per_producer;
    let inner = Arc::new(MutexInner::new());
    let producers_done = Arc::new(AtomicBool::new(false));

    let consumer_inner = Arc::clone(&inner);
    let consumer_done = Arc::clone(&producers_done);
    let consumer = thread::spawn(move || loop {
        consumer_inner
            .consumer_lock_count
            .fetch_add(1, Ordering::SeqCst);
        let mut queue = consumer_inner.queue.lock().expect("queue mutex poisoned");
        let mut processed_now = 0;
        for _ in 0..s.consumer_budget {
            if queue.pop_front().is_some() {
                consumer_inner.processed.fetch_add(1, Ordering::SeqCst);
                processed_now += 1;
            } else {
                break;
            }
        }

        if processed_now > 0 {
            spin_n(s.consumer_hold_spin);
        }

        let processed = consumer_inner.processed.load(Ordering::SeqCst);
        if processed >= total_ops {
            break;
        }

        if consumer_done.load(Ordering::SeqCst) && queue.is_empty() {
            break;
        }
        drop(queue);
        thread::yield_now();
    });

    let start = Instant::now();
    let mut handles = Vec::with_capacity(s.producers);
    for p in 0..s.producers {
        let inner = Arc::clone(&inner);
        handles.push(thread::spawn(move || {
            for i in 0..s.ops_per_producer {
                inner.producer_lock_count.fetch_add(1, Ordering::SeqCst);
                let mut queue = inner.queue.lock().expect("queue mutex poisoned");
                let id = inner.next_id.fetch_add(1, Ordering::SeqCst);
                queue.push_back(id);
                drop(queue);

                if s.producer_pause_every != 0 && (i + p) % s.producer_pause_every == 0 {
                    thread::yield_now();
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("producer panicked");
    }
    producers_done.store(true, Ordering::SeqCst);
    consumer.join().expect("consumer panicked");

    let elapsed = start.elapsed();

    let processed = inner.processed.load(Ordering::SeqCst);
    let queue_left = inner.queue.lock().expect("queue mutex poisoned").len();
    assert_eq!(processed, total_ops, "mutex processed mismatch");
    assert_eq!(queue_left, 0, "mutex queue should be empty");

    MutexResult {
        elapsed,
        throughput_mops: total_ops as f64 / elapsed.as_secs_f64() / 1_000_000.0,
        lock_acquires_producer: inner.producer_lock_count.load(Ordering::SeqCst),
        lock_acquires_consumer: inner.consumer_lock_count.load(Ordering::SeqCst),
    }
}

fn run_scenario(s: Scenario) {
    println!("\n=== Scenario: {} ===", s.name);
    println!(
        "producers={}, ops/producer={}, budget={}, hold_spin={}",
        s.producers, s.ops_per_producer, s.consumer_budget, s.consumer_hold_spin
    );

    let soft = run_softlock_bench(s);
    let mutex = run_mutex_bench(s);

    let speedup = soft.throughput_mops / mutex.throughput_mops;

    println!(
        "SoftLock: elapsed={:.2?}, throughput={:.3} Mops/s",
        soft.elapsed, soft.throughput_mops
    );
    println!(
        "          full={}, pend={}, drained_items={}, drained_batches={}, avg_batch={:.2}",
        soft.full_entries,
        soft.pend_entries,
        soft.drained_items,
        soft.drained_batches,
        soft.avg_batch
    );
    println!(
        "Mutex   : elapsed={:.2?}, throughput={:.3} Mops/s",
        mutex.elapsed, mutex.throughput_mops
    );
    println!(
        "          producer_lock_acq={}, consumer_lock_acq={}",
        mutex.lock_acquires_producer, mutex.lock_acquires_consumer
    );
    println!("SoftLock/Mutex throughput ratio = {:.2}x", speedup);
}

fn main() {
    let scenarios = [
        Scenario {
            name: "Low contention, tiny critical work",
            producers: 2,
            ops_per_producer: 200_000,
            producer_pause_every: 1024,
            consumer_budget: 64,
            consumer_hold_spin: 0,
        },
        Scenario {
            name: "High contention, tiny critical work",
            producers: 8,
            ops_per_producer: 200_000,
            producer_pause_every: 0,
            consumer_budget: 64,
            consumer_hold_spin: 0,
        },
        Scenario {
            name: "High contention, heavy full-holder critical section",
            producers: 8,
            ops_per_producer: 200_000,
            producer_pause_every: 0,
            consumer_budget: 64,
            consumer_hold_spin: 700,
        },
    ];

    println!("SoftLock vs Mutex benchmark (throughput only)");
    println!("Note: This excludes interrupt-latency semantics by design.");
    for s in scenarios {
        run_scenario(s);
    }
}
