use std::time::Duration;

use tasc::{BlockingTaskHandle, TaskBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    global()?;
    local()?;
    Ok(())
}

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

    Ok(())
}

fn local() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = tasc::StdContext::new_blocking(100);
    let task0 = TaskBuilder::with_context(&ctx).spawn_blocking(|_id| {
        println!("task0");
        std::thread::sleep(Duration::from_secs(10));
        let mut i = 0u128;
        #[allow(clippy::unit_arg)]
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        println!("task 0 finished");
        i
    });
    let task1 = TaskBuilder::with_context(&ctx).spawn_blocking(|_id| {
        println!("task1");
        let mut i = 0u128;
        #[allow(clippy::unit_arg)]
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        println!("task 1 finished");
        i
    });
    let task2 = TaskBuilder::with_context(&ctx).spawn_blocking(|_id| {
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
