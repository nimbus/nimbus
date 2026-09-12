#![cfg(test)]
#![cfg(feature = "std")]
use super::*;
use core;
use enc::encode::BrotliEncoderParameter;
use std::alloc::{alloc, dealloc, Layout};
use std::collections::HashMap;
use std::sync::Mutex;
use std::vec::Vec;

#[derive(Default)]
struct CustomAllocatorTrace {
    allocations: Mutex<HashMap<usize, Layout>>,
    frees: Mutex<Vec<(usize, usize)>>,
}

extern "C" fn tracking_alloc(opaque: *mut c_void, size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size.max(1), 64).expect("tracking layout should be valid");
    let ptr = unsafe { alloc(layout) };
    assert!(!ptr.is_null(), "tracking allocation should succeed");
    let trace = unsafe { &*(opaque as *const CustomAllocatorTrace) };
    trace
        .allocations
        .lock()
        .expect("allocation trace should lock")
        .insert(ptr as usize, layout);
    ptr.cast()
}

extern "C" fn tracking_free(opaque: *mut c_void, ptr: *mut c_void) {
    let trace = unsafe { &*(opaque as *const CustomAllocatorTrace) };
    let layout = trace
        .allocations
        .lock()
        .expect("allocation trace should lock")
        .remove(&(ptr as usize))
        .expect("freed pointer should come from the tracking allocator");
    trace
        .frees
        .lock()
        .expect("free trace should lock")
        .push((opaque as usize, ptr as usize));
    unsafe { dealloc(ptr.cast(), layout) };
}

fn assert_container_was_freed(
    trace: &CustomAllocatorTrace,
    opaque: *mut c_void,
    container: *mut c_void,
) {
    assert!(
        trace
            .frees
            .lock()
            .expect("free trace should lock")
            .contains(&(opaque as usize, container as usize)),
        "destroy should free the container with its original allocator opaque"
    );
    assert!(
        trace
            .allocations
            .lock()
            .expect("allocation trace should lock")
            .is_empty(),
        "destroy should release every custom allocation"
    );
}

#[test]
fn destroy_encoder_instance_uses_moved_allocators_opaque() {
    let mut trace = CustomAllocatorTrace::default();
    let opaque = (&mut trace as *mut CustomAllocatorTrace).cast::<c_void>();
    let state = unsafe {
        super::super::compressor::BrotliEncoderCreateInstance(
            Some(tracking_alloc),
            Some(tracking_free),
            opaque,
        )
    };
    assert!(!state.is_null(), "encoder state should be allocated");

    unsafe { super::super::compressor::BrotliEncoderDestroyInstance(state) };

    assert_container_was_freed(&trace, opaque, state.cast());
}

#[test]
fn destroy_work_pool_uses_moved_allocators_opaque() {
    let mut trace = CustomAllocatorTrace::default();
    let opaque = (&mut trace as *mut CustomAllocatorTrace).cast::<c_void>();
    let work_pool = unsafe {
        BrotliEncoderCreateWorkPool(2, Some(tracking_alloc), Some(tracking_free), opaque)
    };
    assert!(!work_pool.is_null(), "work pool should be allocated");

    unsafe { BrotliEncoderDestroyWorkPool(work_pool) };

    assert_container_was_freed(&trace, opaque, work_pool.cast());
}

#[test]
fn test_compress_workpool() {
    let input = [
        102, 114, 111, 109, 32, 99, 116, 121, 112, 101, 115, 32, 105, 109, 112, 111, 114, 116, 32,
        42, 10, 10, 99, 108, 97, 115, 115, 32, 69, 110, 117, 109, 84, 121, 112, 101, 40, 116, 121,
        112, 101, 40, 99, 95, 117, 105, 110, 116, 41, 41, 58, 10, 32, 32, 32, 32, 100, 101, 102,
        32, 95, 95, 110, 101, 119, 95, 95, 40, 109, 101, 116, 97, 99, 108, 115, 41, 58, 10, 32, 32,
        32, 32, 32, 32, 32, 32, 112, 97, 115, 115, 10,
    ];
    let params = [
        BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
        BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
        BrotliEncoderParameter::BROTLI_PARAM_SIZE_HINT,
        BrotliEncoderParameter::BROTLI_PARAM_CATABLE,
        BrotliEncoderParameter::BROTLI_PARAM_MAGIC_NUMBER,
        BrotliEncoderParameter::BROTLI_PARAM_Q9_5,
    ];
    let values = [11u32, 16, 91, 0, 0, 0];
    let mut encoded_size = BrotliEncoderMaxCompressedSizeMulti(input.len(), 4);
    let mut encoded_backing = [0u8; 145];
    let encoded = &mut encoded_backing[..encoded_size];
    let ret = unsafe {
        let wp = BrotliEncoderCreateWorkPool(8, None, None, core::ptr::null_mut());
        let inner_ret = BrotliEncoderCompressWorkPool(
            wp,
            params.len(),
            params[..].as_ptr(),
            values[..].as_ptr(),
            input.len(),
            input[..].as_ptr(),
            &mut encoded_size,
            encoded.as_mut_ptr(),
            4,
            None,
            None,
            core::ptr::null_mut(),
        );
        BrotliEncoderDestroyWorkPool(wp);
        inner_ret
    };
    assert_eq!(ret, 1);
    let mut rt_size = 256;
    let mut rt_buffer = [0u8; 256];
    let ret2 = unsafe {
        super::super::decompressor::CBrotliDecoderDecompress(
            encoded_size,
            encoded.as_ptr(),
            &mut rt_size,
            rt_buffer.as_mut_ptr(),
        )
    };
    match ret2 {
    super::super::decompressor::ffi::interface::BrotliDecoderResult::BROTLI_DECODER_RESULT_SUCCESS => {
    },
    _ => panic!("{}", ret2 as i32),
  }
    assert_eq!(rt_size, input.len());
    assert_eq!(&rt_buffer[..rt_size], &input[..]);
}

#[test]
fn test_compress_empty_workpool() {
    let input = [];
    let params = [
        BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
        BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
        BrotliEncoderParameter::BROTLI_PARAM_SIZE_HINT,
        BrotliEncoderParameter::BROTLI_PARAM_CATABLE,
        BrotliEncoderParameter::BROTLI_PARAM_MAGIC_NUMBER,
        BrotliEncoderParameter::BROTLI_PARAM_Q9_5,
    ];
    let values = [3u32, 16, 91, 0, 0, 0];
    let mut encoded_size = BrotliEncoderMaxCompressedSizeMulti(input.len(), 4);
    let mut encoded_backing = [0u8; 145];
    let encoded = &mut encoded_backing[..encoded_size];
    let ret = unsafe {
        let wp = BrotliEncoderCreateWorkPool(8, None, None, core::ptr::null_mut());
        let inner_ret = BrotliEncoderCompressWorkPool(
            wp,
            params.len(),
            params[..].as_ptr(),
            values[..].as_ptr(),
            input.len(),
            input[..].as_ptr(),
            &mut encoded_size,
            encoded.as_mut_ptr(),
            4,
            None,
            None,
            core::ptr::null_mut(),
        );
        BrotliEncoderDestroyWorkPool(wp);
        inner_ret
    };
    assert_eq!(ret, 1);
    let mut rt_size = 256;
    let mut rt_buffer = [0u8; 256];
    assert_ne!(encoded_size, 0);
    let ret2 = unsafe {
        super::super::decompressor::CBrotliDecoderDecompress(
            encoded_size,
            encoded.as_ptr(),
            &mut rt_size,
            rt_buffer.as_mut_ptr(),
        )
    };
    match ret2 {
    super::super::decompressor::ffi::interface::BrotliDecoderResult::BROTLI_DECODER_RESULT_SUCCESS => {
    },
    _ => panic!("{}", ret2 as i32),
  }
    assert_eq!(rt_size, input.len());
    assert_eq!(&rt_buffer[..rt_size], &input[..]);
}

#[test]
fn test_compress_empty_multi_raw() {
    let input = [];
    let params = [
        BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
        BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
        BrotliEncoderParameter::BROTLI_PARAM_SIZE_HINT,
        BrotliEncoderParameter::BROTLI_PARAM_CATABLE,
        BrotliEncoderParameter::BROTLI_PARAM_MAGIC_NUMBER,
        BrotliEncoderParameter::BROTLI_PARAM_Q9_5,
    ];
    let values = [3u32, 16, 0, 0, 0, 0];
    let mut encoded_size = BrotliEncoderMaxCompressedSizeMulti(input.len(), 4);
    let mut encoded_backing = [0u8; 145];
    let encoded = &mut encoded_backing[..encoded_size];
    let ret = unsafe {
        BrotliEncoderCompressMulti(
            params.len(),
            params[..].as_ptr(),
            values[..].as_ptr(),
            input.len(),
            input[..].as_ptr(),
            &mut encoded_size,
            encoded.as_mut_ptr(),
            4,
            None,
            None,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(ret, 1);
    let mut rt_size = 256;
    let mut rt_buffer = [0u8; 256];
    assert_ne!(encoded_size, 0);
    let ret2 = unsafe {
        super::super::decompressor::CBrotliDecoderDecompress(
            encoded_size,
            encoded.as_ptr(),
            &mut rt_size,
            rt_buffer.as_mut_ptr(),
        )
    };
    match ret2 {
    super::super::decompressor::ffi::interface::BrotliDecoderResult::BROTLI_DECODER_RESULT_SUCCESS => {
    },
    _ => panic!("{}", ret2 as i32),
  }
    assert_eq!(rt_size, input.len());
    assert_eq!(&rt_buffer[..rt_size], &input[..]);
}

#[test]
fn test_compress_null_multi_raw() {
    let params = [
        BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
        BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
        BrotliEncoderParameter::BROTLI_PARAM_SIZE_HINT,
        BrotliEncoderParameter::BROTLI_PARAM_CATABLE,
        BrotliEncoderParameter::BROTLI_PARAM_MAGIC_NUMBER,
        BrotliEncoderParameter::BROTLI_PARAM_Q9_5,
    ];
    let values = [3u32, 16, 0, 0, 0, 0];
    let mut encoded_size = BrotliEncoderMaxCompressedSizeMulti(0, 4);
    let mut encoded_backing = [0u8; 145];
    let encoded = &mut encoded_backing[..encoded_size];
    let ret = unsafe {
        BrotliEncoderCompressMulti(
            params.len(),
            params[..].as_ptr(),
            values[..].as_ptr(),
            0,
            core::ptr::null(),
            &mut encoded_size,
            encoded.as_mut_ptr(),
            4,
            None,
            None,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(ret, 1);
    let mut rt_size = 256;
    let mut rt_buffer = [0u8; 256];
    assert_ne!(encoded_size, 0);
    let ret2 = unsafe {
        super::super::decompressor::CBrotliDecoderDecompress(
            encoded_size,
            encoded.as_ptr(),
            &mut rt_size,
            rt_buffer.as_mut_ptr(),
        )
    };
    match ret2 {
    super::super::decompressor::ffi::interface::BrotliDecoderResult::BROTLI_DECODER_RESULT_SUCCESS => {
    },
    _ => panic!("{}", ret2 as i32),
  }
    assert_eq!(rt_size, 0);
}

#[test]
fn test_compress_empty_multi_raw_one_thread() {
    let input = [];
    let params = [
        BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
        BrotliEncoderParameter::BROTLI_PARAM_Q9_5,
        BrotliEncoderParameter::BROTLI_PARAM_CATABLE,
        BrotliEncoderParameter::BROTLI_PARAM_APPENDABLE,
        BrotliEncoderParameter::BROTLI_PARAM_MAGIC_NUMBER,
    ];
    let values = [10u32, 1, 1, 1, 1];
    let mut encoded_size = BrotliEncoderMaxCompressedSizeMulti(input.len(), 1);
    let mut encoded_backing = [0u8; 25];
    let encoded = &mut encoded_backing[..encoded_size];
    assert_eq!(params.len(), 5);
    assert_eq!(encoded_size, 25);
    let ret = unsafe {
        BrotliEncoderCompressMulti(
            params.len(),
            params[..].as_ptr(),
            values[..].as_ptr(),
            input.len(),
            input[..].as_ptr(),
            &mut encoded_size,
            encoded.as_mut_ptr(),
            1,
            None,
            None,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(ret, 1);
    let mut rt_size = 256;
    let mut rt_buffer = [0u8; 256];
    assert_ne!(encoded_size, 0);
    let ret2 = unsafe {
        super::super::decompressor::CBrotliDecoderDecompress(
            encoded_size,
            encoded.as_ptr(),
            &mut rt_size,
            rt_buffer.as_mut_ptr(),
        )
    };
    match ret2 {
    super::super::decompressor::ffi::interface::BrotliDecoderResult::BROTLI_DECODER_RESULT_SUCCESS => {
    },
    _ => panic!("{}", ret2 as i32),
  }
    assert_eq!(rt_size, input.len());
    assert_eq!(&rt_buffer[..rt_size], &input[..]);
}

#[test]
fn test_compress_empty_multi_catable() {
    let input = [];
    let params = [
        BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
        BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
        BrotliEncoderParameter::BROTLI_PARAM_SIZE_HINT,
        BrotliEncoderParameter::BROTLI_PARAM_CATABLE,
        BrotliEncoderParameter::BROTLI_PARAM_MAGIC_NUMBER,
        BrotliEncoderParameter::BROTLI_PARAM_Q9_5,
    ];
    let values = [3u32, 16, 0, 1, 1, 0];
    let mut encoded_size = BrotliEncoderMaxCompressedSizeMulti(input.len(), 4);
    let mut encoded_backing = [0u8; 145];
    let encoded = &mut encoded_backing[..encoded_size];
    let ret = unsafe {
        BrotliEncoderCompressMulti(
            params.len(),
            params[..].as_ptr(),
            values[..].as_ptr(),
            input.len(),
            input[..].as_ptr(),
            &mut encoded_size,
            encoded.as_mut_ptr(),
            4,
            None,
            None,
            core::ptr::null_mut(),
        )
    };
    assert_eq!(ret, 1);
    let mut rt_size = 256;
    let mut rt_buffer = [0u8; 256];
    assert_ne!(encoded_size, 0);
    let ret2 = unsafe {
        super::super::decompressor::CBrotliDecoderDecompress(
            encoded_size,
            encoded.as_ptr(),
            &mut rt_size,
            rt_buffer.as_mut_ptr(),
        )
    };
    match ret2 {
    super::super::decompressor::ffi::interface::BrotliDecoderResult::BROTLI_DECODER_RESULT_SUCCESS => {
    },
    _ => panic!("{:?}", ret2 as i32),
  }
    assert_eq!(rt_size, input.len());
    assert_eq!(&rt_buffer[..rt_size], &input[..]);
}
