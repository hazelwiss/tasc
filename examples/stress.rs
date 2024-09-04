use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

const EXPENSE: u64 = 1_000_000_000;

#[inline(never)]
fn expensive() {
    let mut i = 0;
    for _ in 0..EXPENSE {
        i = std::hint::black_box(i + 1);
    }
}

fn main() {
    let thread_count = num_cpus::get();
    println!("thread amount of cores: {thread_count}\n");

    let mut w0 = vec![];
    let mut w1 = vec![];
    let mut w2 = vec![];
    let mut w3 = vec![];

    // thread count workers
    w0.push((thread_count, perform_tasks(thread_count, 25)));
    w1.push((thread_count, perform_tasks(thread_count, 50)));
    w2.push((thread_count, perform_tasks(thread_count, 75)));
    w3.push((thread_count, perform_tasks(thread_count, 100)));

    // thread count * 2 workers
    w0.push((thread_count * 2, perform_tasks(thread_count * 2, 25)));
    w1.push((thread_count * 2, perform_tasks(thread_count * 2, 50)));
    w2.push((thread_count * 2, perform_tasks(thread_count * 2, 75)));
    w3.push((thread_count * 2, perform_tasks(thread_count * 2, 100)));

    // 4 workers
    w0.push((4, perform_tasks(4, 25)));
    w1.push((4, perform_tasks(4, 50)));
    w2.push((4, perform_tasks(4, 75)));
    w3.push((4, perform_tasks(4, 100)));

    // 8 workers
    w0.push((8, perform_tasks(8, 25)));
    w1.push((8, perform_tasks(8, 50)));
    w2.push((8, perform_tasks(8, 75)));
    w3.push((8, perform_tasks(8, 100)));

    // 16 workers
    w0.push((16, perform_tasks(16, 25)));
    w1.push((16, perform_tasks(16, 50)));
    w2.push((16, perform_tasks(16, 75)));
    w3.push((16, perform_tasks(16, 100)));

    // 32 workers
    w0.push((32, perform_tasks(32, 25)));
    w1.push((32, perform_tasks(32, 50)));
    w2.push((32, perform_tasks(32, 75)));
    w3.push((32, perform_tasks(32, 100)));

    // 128 workers
    w0.push((128, perform_tasks(128, 25)));
    w1.push((128, perform_tasks(128, 50)));
    w2.push((128, perform_tasks(128, 75)));
    w3.push((128, perform_tasks(128, 100)));

    // 256 workers
    w0.push((256, perform_tasks(256, 25)));
    w1.push((256, perform_tasks(256, 50)));
    w2.push((256, perform_tasks(256, 75)));
    w3.push((256, perform_tasks(256, 100)));

    // 1024 workers
    w0.push((1024, perform_tasks(1024, 25)));
    w1.push((1024, perform_tasks(1024, 50)));
    w2.push((1024, perform_tasks(1024, 75)));
    w3.push((1024, perform_tasks(1024, 100)));

    // best total, avg for 25 work
    let tot = w0
        .iter()
        .min_by(|&&e, &&o| e.1 .0.total_cmp(&o.1 .0))
        .unwrap();
    let avg = w0
        .iter()
        .min_by(|&&e, &&o| e.1 .1.total_cmp(&o.1 .1))
        .unwrap();
    println!(
        "work: 25, best total: [{}] {:.02}, best avg: [{}] {:.05}",
        tot.0, tot.1 .0, avg.0, avg.1 .1
    );

    // best total, avg for 50 work
    let tot = w1
        .iter()
        .min_by(|&&e, &&o| e.1 .0.total_cmp(&o.1 .0))
        .unwrap();
    let avg = w1
        .iter()
        .min_by(|&&e, &&o| e.1 .1.total_cmp(&o.1 .1))
        .unwrap();
    println!(
        "work: 50, best total: [{}] {:.02}, best avg: [{}] {:.05}",
        tot.0, tot.1 .0, avg.0, avg.1 .1
    );

    // best total, avg for 75 work
    let tot = w2
        .iter()
        .min_by(|&&e, &&o| e.1 .0.total_cmp(&o.1 .0))
        .unwrap();
    let avg = w2
        .iter()
        .min_by(|&&e, &&o| e.1 .1.total_cmp(&o.1 .1))
        .unwrap();
    println!(
        "work: 75, best total: [{}] {:.02}, best avg: [{}] {:.05}",
        tot.0, tot.1 .0, avg.0, avg.1 .1
    );

    // best total, avg for 100 work
    let tot = w3
        .iter()
        .min_by(|&&e, &&o| e.1 .0.total_cmp(&o.1 .0))
        .unwrap();
    let avg = w3
        .iter()
        .min_by(|&&e, &&o| e.1 .1.total_cmp(&o.1 .1))
        .unwrap();
    println!(
        "work: 100, best total: [{}] {:.02}, best avg: [{}] {:.05}",
        tot.0, tot.1 .0, avg.0, avg.1 .1
    );
}

fn perform_tasks(workers: usize, work: usize) -> (f64, f64) {
    println!("workers: {workers}, work: {work}");

    let ctx = tasc::StdContext::new_blocking(workers);
    let start = Arc::new(AtomicBool::new(false));

    let mut handlers = Vec::with_capacity(workers);

    let work_per_worker = work / workers;
    let mut remainder = work % workers;
    for _ in 0..workers {
        let worker_load = work_per_worker + if remainder > 0 { 1 } else { 0 };
        let start = start.clone();
        handlers.push(
            tasc::TaskBuilder::<_, tasc::global::Signal>::from_ctx(&ctx).spawn_blocking(
                move |_| {
                    while !start.load(Ordering::Acquire) {
                        // we want it to busy wait
                        std::hint::black_box(())
                    }
                    let start = Instant::now();
                    for _ in 0..worker_load {
                        #[allow(clippy::unit_arg)]
                        std::hint::black_box(expensive());
                    }
                    Instant::now() - start
                },
            ),
        );
        remainder = remainder.saturating_sub(1);
    }
    let total_time = Instant::now();
    start.store(true, Ordering::Release);
    let all_workers_time = handlers
        .into_iter()
        .map(|h| h.wait_blocking().unwrap().as_secs_f64())
        .sum::<f64>();

    let total_time = (Instant::now() - total_time).as_secs_f64();
    let avg_time = all_workers_time / workers as f64;

    println!("total time: {total_time:.02}, avg time: {avg_time:.05}\n");
    (total_time, avg_time)
}
