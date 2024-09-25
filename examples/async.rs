fn main() {
    pollster::block_on(run()).unwrap()
}

async fn run() -> tasc::error::Result<()> {
    tasc::task(async { println!("I am an async task!") }).await?;

    let value = tasc::task(async { 2 + 2 }).await.unwrap();
    println!("returned {value} from async task!");

    let mut borrowed = [1, 2, 3];
    let handle = tasc::scoped(async {
        borrowed[0] = 3;
        borrowed[2] = 1;
    })
    .await;
    // using `borrowed` here would cause a compile error since it is mutably borrowed by the handle!
    handle?;
    borrowed[1] = 5;
    assert_eq!(borrowed, [3, 5, 1]);

    Ok(())
}
