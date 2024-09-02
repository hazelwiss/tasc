use tasc::{BlockingTaskHandle, TaskBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    global()?;
    local()?;
    Ok(())
}

fn global() -> Result<(), Box<dyn std::error::Error>> {
    println!("beginning global");

    tasc::blocking::task(|_id| {}).wait_blocking()?;

    let mut x = vec![];
    tasc::blocking::scoped(|_id| {
        x.push(2);
        x.push(3);
    })
    .wait_blocking()?;

    let task = tasc::blocking::scoped(|_id| {
        println!("{x:?}");
    });
    task.wait_blocking()?;

    Ok(())
}

fn local() -> Result<(), Box<dyn std::error::Error>> {
    println!("beggining local");

    let ctx = tasc::StdContext::new_blocking(100);
    let task0 = TaskBuilder::with_context(&ctx).spawn_blocking(|_id| {
        println!("task0");
        let mut i = 0u128;
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        i
    });
    let task1 = TaskBuilder::with_context(&ctx).spawn_blocking(|_id| {
        println!("task1");
        let mut i = 0u128;
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        i
    });
    let task2 = TaskBuilder::with_context(&ctx).spawn_blocking(|_id| {
        println!("task2");
        let mut i = 0u128;
        std::hint::black_box(for _ in 0..100000 {
            i += 1;
        });
        i
    });

    println!("waiting for tasks!");
    println!("task0: {}", task0.wait_blocking()?);
    println!("task1: {}", task1.wait_blocking()?);
    println!("task2: {}", task2.wait_blocking()?);
    Ok(())
}
