use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    global()?;
    local()?;
    Ok(())
}

#[inline(never)]
fn global() -> Result<(), Box<dyn std::error::Error>> {
    tasc::blocking::task(|_id| {}).wait_blocking()?;

    let mut x = vec![0; 200];
    let (a, b) = x.split_at_mut(2);
    let (c, d) = b.split_at_mut(5);
    let task0 = tasc::blocking::scoped(|_id| {
        for e in a {
            *e += 1;
        }
    });
    let task1 = tasc::blocking::scoped(|_id| {
        for e in c {
            *e += 1;
        }
        for e in d {
            *e += 1;
        }
    });

    task0.wait_blocking()?;
    task1.wait_blocking()?;

    assert_eq!(x.iter().sum::<u32>(), 200);

    #[inline(never)]
    fn sum(start: u64, end: u64) -> u64 {
        let mut sum = 0;
        for v in start..end {
            sum = std::hint::black_box(sum + v);
        }
        sum
    }

    println!("summing large numbers");
    let n0 = 10_000_000_000;
    let n1 = 20_000_000_000;
    let n2 = 30_000_000_000;
    let sum_0_to_n0 = tasc::blocking::task(move |_| {
        let start = Instant::now();
        let res = sum(0, n0);
        println!(
            "summed 0 - {n0}, time: {:.02}",
            (Instant::now() - start).as_secs_f64()
        );
        res
    });
    let sum_0_to_n1 = tasc::blocking::task(move |_| {
        let start = Instant::now();
        let res = sum(0, n1);
        println!(
            "summed 0 - {n1}, time: {:.02}",
            (Instant::now() - start).as_secs_f64()
        );
        res
    });
    let sum_0_to_n2 = tasc::blocking::task(move |_| {
        let start = Instant::now();
        let res = sum(0, n2);
        println!(
            "summed 0 - {n2}, time: {:.02}",
            (Instant::now() - start).as_secs_f64()
        );
        res
    });
    let sum: tasc::error::Result<u64> = *tasc::blocking::task(|_| {
        Ok(*sum_0_to_n2.wait_blocking()?
            + *sum_0_to_n1.wait_blocking()?
            + *sum_0_to_n0.wait_blocking()?)
    })
    .wait_blocking()?;
    let sum = sum?;
    println!("sum: {sum}");
    assert_eq!(
        sum,
        (0..n0).sum::<u64>() + (0..n1).sum::<u64>() + (0..n2).sum::<u64>()
    );

    Ok(())
}

#[inline(never)]
fn local() -> Result<(), Box<dyn std::error::Error>> {
    type TaskBuilder<'a> = tasc::TaskBuilder<'a, tasc::StdContext, tasc::StdSignal>;

    let ctx = tasc::StdContext::new_blocking(100);
    let task0 = TaskBuilder::from_ctx(&ctx).spawn_blocking(|_id| {
        println!("task0");
        std::thread::sleep(Duration::from_secs(3));
        let mut i = 0u128;
        #[allow(clippy::unit_arg)]
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        println!("task 0 finished");
        i
    });
    let task1 = TaskBuilder::from_ctx(&ctx).spawn_blocking(|_id| {
        println!("task1");
        let mut i = 0u128;
        #[allow(clippy::unit_arg)]
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        println!("task 1 finished");
        i
    });
    let task2 = TaskBuilder::from_ctx(&ctx).spawn_blocking(|_id| {
        println!("task2");
        let mut i = 0u128;
        #[allow(clippy::unit_arg)]
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        println!("task 2 finished");
        i
    });

    println!("task0: {}", task0.wait_blocking()?);
    println!("task1: {}", task1.wait_blocking()?);
    println!("task2: {}", task2.wait_blocking()?);
    Ok(())
}
