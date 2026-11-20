// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The clips on the audio thread with clips to play, which the contract
//! suite's real-time checks (a track without clips) don't reach: nothing
//! is allocated while they play, at 100 % and away from it, through
//! seeks, a change of speed and a stop.
//!
//! Not with the `oxp` feature: the plugin file's own counting allocator
//! (`export_plugin!`) is then the program's.
#![cfg(not(feature = "oxp"))]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use oxiguitar_clips_plugin::sound::{Clips, Segment};

/// Counts this thread's allocations while `COUNTING` is on.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

// SAFETY: forwards every call to the system's allocator unchanged.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn playing_clips_allocates_nothing() {
    let rate = 48_000u32;
    let r = f64::from(rate);
    let sound: Vec<f32> = (0..3 * rate as usize)
        .flat_map(|i| {
            let s = (i as f64 * 220.0 * std::f64::consts::TAU / r).sin() as f32 * 0.4;
            [s, s]
        })
        .collect();
    let sound = Arc::new(sound);
    let mut clips = Clips::new(
        vec![
            Segment {
                start: rate as i64 / 2,
                sound: sound.clone(),
            },
            Segment {
                start: 5 * rate as i64,
                sound,
            },
        ],
        rate,
        512,
    );
    let (mut left, mut right) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    let mut play = |clips: &mut Clips, from: f64, speed: f64, calls: usize| {
        let step = speed / r;
        let mut time = from;
        for _ in 0..calls {
            clips.process_at(&mut left, &mut right, time, step);
            time += 512.0 * step;
        }
    };
    COUNTING.with(|c| c.set(true));
    play(&mut clips, 0.0, 1.0, 300);
    play(&mut clips, 1.0, 0.75, 300);
    play(&mut clips, 4.5, 0.75, 300);
    play(&mut clips, 6.0, 1.5, 100);
    play(&mut clips, 0.0, 0.0, 10);
    play(&mut clips, 2.0, 0.5, 200);
    COUNTING.with(|c| c.set(false));
    assert_eq!(COUNT.with(Cell::get), 0, "allocations while playing");
}
