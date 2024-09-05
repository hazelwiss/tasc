fn main() {
    pollster::block_on(run()).unwrap()
}

async fn run() -> tasc::error::Result<()> {
    tasc::task(|_| println!("I am an async task!"))
        .await
        .await?;

    let value = tasc::task(|_| 2 + 2).await.await.unwrap();
    println!("returned {value} from async task!");

    let mut borrowed = [1, 2, 3];
    let handle = tasc::scoped(|_| {
        borrowed[0] = 3;
        borrowed[2] = 1;
    })
    .await;
    // using `borrowed` here would cause a compile error since it is mutably borrowed by the handle!
    handle.await?;
    borrowed[1] = 5;
    assert_eq!(borrowed, [3, 5, 1]);

    Ok(())
}
