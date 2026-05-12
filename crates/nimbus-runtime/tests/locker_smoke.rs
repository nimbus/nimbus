//! Phase 4: Smoke tests for V8 Locker API via forked rusty_v8.
//! Only compiles when the [patch.crates-io] fork is active.

use std::pin::pin;
use std::sync::mpsc;
use std::thread;

/// Helper: initialize V8 platform via deno_core (required before any V8 usage).
fn ensure_v8_init() {
    let _rt = deno_core::JsRuntime::new(Default::default());
}

#[test]
fn unentered_isolate_is_send() {
    ensure_v8_init();
    let isolate = v8::Isolate::new_unentered(v8::CreateParams::default());

    // UnenteredIsolate implements Send — can be transferred across threads
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // Use it on the other thread
        let mut iso = isolate;
        let mut locker = v8::Locker::new(&mut iso);
        let scope = pin!(v8::HandleScope::new(&mut *locker));
        let scope = &mut scope.init();
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        let code = v8::String::new(scope, "42").unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        let result = script.run(scope).unwrap();
        tx.send(result.int32_value(scope).unwrap()).unwrap();
    })
    .join()
    .unwrap();

    assert_eq!(rx.recv().unwrap(), 42);
}

#[test]
fn locker_provides_isolate_access() {
    ensure_v8_init();
    let mut isolate = v8::Isolate::new_unentered(v8::CreateParams::default());

    let mut locker = v8::Locker::new(&mut isolate);
    let scope = pin!(v8::HandleScope::new(&mut *locker));
    let scope = &mut scope.init();
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    let code = v8::String::new(scope, "'hello from locker'").unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap();
    let result = script.run(scope).unwrap();
    assert!(result.is_string());
}

#[test]
fn sequential_lock_unlock_across_threads() {
    ensure_v8_init();
    let mut isolate = v8::Isolate::new_unentered(v8::CreateParams::default());

    // Thread A: lock, execute, unlock
    {
        let mut locker = v8::Locker::new(&mut isolate);
        let scope = pin!(v8::HandleScope::new(&mut *locker));
        let scope = &mut scope.init();
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        let code = v8::String::new(scope, "1 + 1").unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        let result = script.run(scope).unwrap();
        assert_eq!(result.int32_value(scope), Some(2));
    }

    // Send to thread B
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut locker = v8::Locker::new(&mut isolate);
        let scope = pin!(v8::HandleScope::new(&mut *locker));
        let scope = &mut scope.init();
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        let code = v8::String::new(scope, "2 + 2").unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        let result = script.run(scope).unwrap();
        tx.send(result.int32_value(scope).unwrap()).unwrap();
    })
    .join()
    .unwrap();

    assert_eq!(rx.recv().unwrap(), 4);
}

#[test]
fn isolate_state_preserved_across_lock_cycles() {
    ensure_v8_init();
    let mut isolate = v8::Isolate::new_unentered(v8::CreateParams::default());

    // Lock cycle 1: create a persistent context with a global variable
    let context_global;
    {
        let mut locker = v8::Locker::new(&mut isolate);
        let scope = pin!(v8::HandleScope::new(&mut *locker));
        let scope = &mut scope.init();
        let context = v8::Context::new(scope, Default::default());
        context_global = v8::Global::new(scope, context);

        let scope = &mut v8::ContextScope::new(scope, context);
        let code = v8::String::new(scope, "globalThis.testValue = 99").unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        script.run(scope).unwrap();
    }

    // Lock cycle 2: verify the global persisted
    {
        let mut locker = v8::Locker::new(&mut isolate);
        let scope = pin!(v8::HandleScope::new(&mut *locker));
        let scope = &mut scope.init();
        let context = v8::Local::new(scope, &context_global);
        let scope = &mut v8::ContextScope::new(scope, context);

        let code = v8::String::new(scope, "globalThis.testValue").unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        let result = script.run(scope).unwrap();
        assert_eq!(result.int32_value(scope), Some(99));
    }
}

#[test]
fn jsruntime_with_use_locker_basic() {
    let mut runtime = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        use_locker: true,
        ..Default::default()
    });

    runtime.execute_script("locker_test.js", "1 + 1").unwrap();
}

#[test]
fn jsruntime_two_locker_runtimes_same_thread() {
    let mut rt1 = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        use_locker: true,
        ..Default::default()
    });
    assert!(rt1.release_v8_lock());

    let mut rt2 = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        use_locker: true,
        ..Default::default()
    });
    assert!(rt2.release_v8_lock());

    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script("rt1_init.js", "globalThis.counter = 1;")
            .unwrap();
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script("rt2_init.js", "globalThis.counter = 10;")
            .unwrap();
    }

    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script(
                "rt1_step.js",
                "if (globalThis.counter !== 1) throw new Error('rt1 lost state'); globalThis.counter += 1;",
            )
            .unwrap();
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script(
                "rt2_step.js",
                "if (globalThis.counter !== 10) throw new Error('rt2 lost state'); globalThis.counter += 5;",
            )
            .unwrap();
    }

    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script(
                "rt1_verify.js",
                "if (globalThis.counter !== 2) throw new Error('rt1 wrong final state');",
            )
            .unwrap();
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script(
                "rt2_verify.js",
                "if (globalThis.counter !== 15) throw new Error('rt2 wrong final state');",
            )
            .unwrap();
    }
}

#[test]
fn jsruntime_mixed_standard_and_locker_same_thread() {
    let mut standard = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        use_locker: false,
        ..Default::default()
    });
    let mut lockable = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        use_locker: true,
        ..Default::default()
    });
    assert!(lockable.release_v8_lock());

    standard
        .execute_script("std_init.js", "globalThis.mode = 'standard';")
        .unwrap();
    {
        let mut locked = lockable.acquire_v8_lock();
        locked
            .execute_script("lock_init.js", "globalThis.mode = 'locker';")
            .unwrap();
    }

    standard
        .execute_script(
            "std_verify.js",
            "if (globalThis.mode !== 'standard') throw new Error('standard runtime lost state');",
        )
        .unwrap();
    {
        let mut locked = lockable.acquire_v8_lock();
        locked
            .execute_script(
                "lock_verify.js",
                "if (globalThis.mode !== 'locker') throw new Error('locker runtime lost state');",
            )
            .unwrap();
    }
}

#[test]
fn two_jsruntimes_interleaved_raw_locker() {
    // The M:1 smoke test: two UnenteredIsolates on one thread,
    // alternating execution. This validates the cooperative scheduling
    // primitive that Phase 5 will build on.
    ensure_v8_init();
    let mut iso_a = v8::Isolate::new_unentered(v8::CreateParams::default());
    let mut iso_b = v8::Isolate::new_unentered(v8::CreateParams::default());

    fn exec(iso: &mut v8::UnenteredIsolate, expr: &str) -> i32 {
        let mut locker = v8::Locker::new(iso);
        let scope = pin!(v8::HandleScope::new(&mut *locker));
        let scope = &mut scope.init();
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        let code = v8::String::new(scope, expr).unwrap();
        let script = v8::Script::compile(scope, code, None).unwrap();
        script.run(scope).unwrap().int32_value(scope).unwrap()
    }

    assert_eq!(exec(&mut iso_a, "1 + 1"), 2);
    assert_eq!(exec(&mut iso_b, "2 + 2"), 4);
    assert_eq!(exec(&mut iso_a, "3 + 3"), 6);
    assert_eq!(exec(&mut iso_b, "4 + 4"), 8);
}
