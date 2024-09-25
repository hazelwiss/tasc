tasc is an asynchronous worker pool library, allowing you to distribute work between multiple workers in both an asynchronous and synchronous API.
tasc aims to be simplistic and portable, the base API does not rely on running within a context that provides the standard library, and can be run in an `no-std` environment.
The API aims to be very similar to that of [`std::thread`], so that if you understand one then understanding the other is simple.

tasc operates, by default, by using a global context which relies on the standard library. The helper functions [`task`], [`scope`] and everything under [`sync`] will use the global context by default, and requires the global feature to be enabled. When the global feature is enabled, it requires enabling the std feature which will require a dependency on the standard library.

# Using Async tasc

tasc is centered around providing an asynchronous API, so therefore the most simplest way to do something is generally asynchronous and not synchronous.
Although a synchronous API is provided, it is secondary to the asynchronous one.

```rust
extern "Rust" {
	fn do_work();
}

async {
	// this will create a task, and them immediately awaits the task to completion.
	// the `.await` is just because creating a task is considered an ascynchronous process.
	tasc::task(async { println!("this is a task running asynchronously") }).await;

	// this will create a task, do some work, then join it at a later point.
	let handle = tasc::task(async { println!("this will run on a separate thread!") });
	unsafe { do_work() };
	handle.await; // here we join the thread

	// a task is capable of returning a value.
	let handle = tasc::task(async {
		let foo = 1;
		let bar = 2;
		foo + bar
	});
	let result = handle.await.unwrap();
};
```

# Scoped Tasks

tasc allows for scoped tasks, which are quite similar to scoped threads in the standard library, except it does not have the [`scope`] function.

```rust
async {
	let mut v = vec![];

	tasc::scoped(async {
		// borrows v mutably
		v.push(2);
	}).await;

	println!("{v:?}");
};
```

there is also a synchronous variant.

```rust
let mut v = vec![];

tasc::sync::scoped(|| {
	// borrows v mutably
	v.push(2);
}).wait();

println!("{v:?}");
```

Scoped tasks are awaited upon drop.

# Using The Synchronous API

tasc does not force using using an asynchronous API, and provides a synchronous alternative such as [`sync::task`] and [`sync::scope`].

```rust
let handle0 = tasc::sync::task(|| 20);
let bar = 5;
let handle1 = tasc::sync::scoped(|| 20 + bar);

let sum = handle0.wait().unwrap() + handle1.wait().unwrap();
println!("{sum:?}");
```

# Using The Context Directly

tasc allows you to create your own context by deriving the [`TaskContext`] trait on a type, and then passing that to [`TaskBuilder`]. If the `std` feature is enabled, tasc will provide such a context already called [`StdContext`]. Using [`StdContext`] outside of the global context is quite simple.

```rust
async {
	// create a context with 8 worker threads.
	let cx = tasc::StdContext::new(8);
	tasc::TaskBuilder::<tasc::StdContext, tasc::global::Signal>::from_ctx(&cx).spawn(async { println!("I am now spawned from the cx context!") });

	// optionally, we can also use the [`Default`] trait if the `std` feature is enabled, which will create a [`TaskBuilder`] using the global context and global signal.
	tasc::TaskBuilder::default().spawn(async { println!("I am created in the global context, prefer to use 'tasc::task' instead!") });
};
```
