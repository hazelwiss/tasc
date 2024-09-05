tasc is an asynchrounous worker pool library, allowing you to distribute work between multiple workers in both an asyncrhronous, and blocking API.
tasc aims to be simplistic and portable, the base API does not rely on running within a context that provides the standard library, and can be run in an `no-std` environment.
The API aims to be very similar to that of `[std::thread]`, so that if you understand one then understanding the other is simple.

tasc operates, by default, by using a global context which relies on the standard library. The helper functions `tasc::task`, `tasc::scope` and everything under `tasc::blocking` will use the global context by default, and requires the global feature to be enabled. When the global feature is enabled, it requires enabling the std feature which will require a dependency on the standard library.

# Using Async tasc

tasc is centered around providing an asynchronous API, so therefore the most simplest way to do something is generally asynchronous and not blocking.
Although a blocking API is provided, it is secondary to the asynchronous one.

```rs
async {
	// this will create a task, and them immediately awaits the task to completion.
	// the `.await` is just because creating a task is considered an ascynchronous process.
	tasc::task(|id| println!("this is a task running asyncrhonously with an id of {id}")).await;

	// this will create a task, do some work, then join it at a later point.
	let handle = tasc::task(|id| println!("this will run on a separate thread!")).await;
	do_work();
	handle.await; // here we join the thread

	// a task is capable of returning a value.
	let handle = tasc::task(|_id| {
		let foo = 1;
		let bar = 2;
		foo + bar
	});
	let result = hadle.handle.await.await.unwrap();
}
```

# Scoped Tasks

tasc allows for scoped tasks, which are quite similar to scoped threads in the standard library, except it does not have the `scope` function.

```rs
async {
	let mut v = vec![];

	tasc::scope(|_id| {
		// borrows v mutably
		v.push(2);
	}).await;

	println!("{x:?}");
}
```

# Using The Blocking API

tasc does not force using using an asynchronous API, and provides a blocking alternative such as `task::blocking::task` and `task::blocking::scope`.

```rs
let handle0 = tasc::blocking::task(|_| 20);
let bar = 5;
let handle1 = tasc::blocking::scope(|_| 20 + bar);

let sum = handle.wait().unwrap() + handle1.wait().unwrap();
println!("{sum:?}");
```

# Using The Context Directly

tasc allows you to create your own context by deriving the `[TaskContext]` trait on a type, and then passing that to `[TaskBuilder]`. If the `std` feature is enabled, tasc will provide such a context already called `[StdContext]`. Using `[StdContext]` outside of the global context is quite simple.

```rs
async {
	// create a context with 8 worker threads.
	let cx = tasc::StdContext::new(8).await;
	tasc::TaskBuilder::<tasc::global::Signal>::from_ctx(&cx).spawn(|_id| println!("I am now spawned from the cx context!")).await;

	// optionally, we can also use the `[Default]` trait if the `std` feature is enabled, which will create a `[TaskBuilder]` using the global context and global signal.
	tasc::TaskBuilder::default().spawn(|_id| println!("I am created in the global context, prefer to use 'tasc::task' instead!")).await;
}
```
