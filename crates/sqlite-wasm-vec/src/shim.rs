use std::cmp::Ordering;
use std::ffi::{c_char, c_double, c_int, c_long, c_void};
use std::ptr;

type c_size_t = usize;

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_assert_fail(
    _expr: *const c_char,
    _file: *const c_char,
    _line: c_int,
    _func: *const c_char,
) {
    std::process::abort();
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_strncmp(
    s1: *const c_char,
    s2: *const c_char,
    n: c_size_t,
) -> c_int {
    for i in 0..n {
        let c1 = *s1.add(i);
        let c2 = *s2.add(i);

        if c1 != c2 || c1 == 0 {
            return (c1 as c_int) - (c2 as c_int);
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_errno_location() -> *mut c_int {
    thread_local! {
        static ERROR_STORAGE: std::cell::UnsafeCell<i32> = std::cell::UnsafeCell::new(0);
    }
    ERROR_STORAGE.with(|e| e.get())
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_atoi(s: *const c_char) -> c_int {
    if s.is_null() {
        return 0;
    }

    let mut ptr = s;
    let mut result: c_int = 0;
    let mut sign = 1;

    while *ptr != 0 && (*ptr as u8).is_ascii_whitespace() {
        ptr = ptr.offset(1);
    }

    if *ptr != 0 {
        match *ptr as u8 {
            b'+' => ptr = ptr.offset(1),
            b'-' => {
                sign = -1;
                ptr = ptr.offset(1);
            }
            _ => {}
        }
    }

    while *ptr != 0 {
        let c = *ptr as u8;

        if c.is_ascii_digit() {
            let digit = (c - b'0') as c_int;

            if result > (c_int::MAX - digit) / 10 {
                return if sign == 1 { c_int::MAX } else { c_int::MIN };
            }

            result = result * 10 + digit;
        } else {
            break;
        }

        ptr = ptr.offset(1);
    }

    result * sign
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_strtol(
    s: *const c_char,
    endptr: *mut *mut c_char,
    base: c_int,
) -> c_long {
    if s.is_null() {
        if !endptr.is_null() {
            *endptr = s as *mut c_char;
        }
        return 0;
    }

    let start = s;
    let mut ptr = s;
    let mut result: c_long = 0;
    let mut sign = 1;
    let mut valid = false;

    while *ptr != 0 && (*ptr as u8).is_ascii_whitespace() {
        ptr = ptr.offset(1);
    }

    if *ptr != 0 {
        match *ptr as u8 {
            b'+' => ptr = ptr.offset(1),
            b'-' => {
                sign = -1;
                ptr = ptr.offset(1);
            }
            _ => {}
        }
    }

    let actual_base = if base == 0 {
        if *ptr != 0 && (*ptr as u8 == b'0') {
            let next_char = *ptr.offset(1) as u8;
            if next_char == b'x' || next_char == b'X' {
                16
            } else {
                8
            }
        } else {
            10
        }
    } else if base >= 2 && base <= 36 {
        base
    } else {
        if !endptr.is_null() {
            *endptr = ptr as *mut c_char;
        }
        return 0;
    };

    if actual_base == 16 && *ptr != 0 && (*ptr as u8 == b'0') {
        let next_char = *ptr.offset(1) as u8;
        if next_char == b'x' || next_char == b'X' {
            ptr = ptr.offset(2);
        }
    }

    while *ptr != 0 {
        let c = *ptr as u8;
        let digit = match c {
            b'0'..=b'9' => (c - b'0') as c_long,
            b'a'..=b'z' => (c - b'a' + 10) as c_long,
            b'A'..=b'Z' => (c - b'A' + 10) as c_long,
            _ => break,
        };

        if digit >= actual_base as c_long {
            break;
        }

        valid = true;

        if result > (c_long::MAX - digit) / actual_base as c_long {
            result = if sign == 1 { c_long::MAX } else { c_long::MIN };
            break;
        }

        result = result * (actual_base as c_long) + digit;
        ptr = ptr.offset(1);
    }

    if !endptr.is_null() {
        *endptr = if valid {
            ptr as *mut c_char
        } else {
            start as *mut c_char
        };
    }

    result * sign
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_strtod(
    s: *const c_char,
    endptr: *mut *mut c_char,
) -> c_double {
    if s.is_null() {
        if !endptr.is_null() {
            *endptr = s as *mut c_char;
        }
        return 0.0;
    }

    let start = s;
    let mut ptr = s;
    let mut sign = 1.0;
    let mut valid = false;

    while *ptr != 0 && (*ptr as u8).is_ascii_whitespace() {
        ptr = ptr.offset(1);
    }

    if *ptr != 0 {
        match *ptr as u8 {
            b'+' => ptr = ptr.offset(1),
            b'-' => {
                sign = -1.0;
                ptr = ptr.offset(1);
            }
            _ => {}
        }
    }

    let mut integer_part = 0.0;
    while *ptr != 0 {
        let c = *ptr as u8;
        if c.is_ascii_digit() {
            valid = true;
            integer_part = integer_part * 10.0 + (c - b'0') as c_double;
        } else {
            break;
        }
        ptr = ptr.offset(1);
    }

    let mut fractional_part = 0.0;
    let mut fractional_weight = 0.1;
    if *ptr != 0 && (*ptr as u8 == b'.') {
        ptr = ptr.offset(1);
        while *ptr != 0 {
            let c = *ptr as u8;
            if c.is_ascii_digit() {
                valid = true;
                fractional_part += (c - b'0') as c_double * fractional_weight;
                fractional_weight *= 0.1;
            } else {
                break;
            }
            ptr = ptr.offset(1);
        }
    }

    let mut exponent_part = 0;
    let mut exponent_sign = 1;
    if *ptr != 0 && ((*ptr as u8 == b'e') || (*ptr as u8 == b'E')) {
        ptr = ptr.offset(1);

        if *ptr != 0 {
            match *ptr as u8 {
                b'+' => ptr = ptr.offset(1),
                b'-' => {
                    exponent_sign = -1;
                    ptr = ptr.offset(1);
                }
                _ => {}
            }
        }

        while *ptr != 0 {
            let c = *ptr as u8;
            if c.is_ascii_digit() {
                exponent_part = exponent_part * 10 + (c - b'0') as c_int;
            } else {
                break;
            }
            ptr = ptr.offset(1);
        }
    }

    if !endptr.is_null() {
        *endptr = if valid {
            ptr as *mut c_char
        } else {
            start as *mut c_char
        };
    }

    if !valid {
        return 0.0;
    }

    let mut result = (integer_part + fractional_part) * sign;

    if exponent_part != 0 {
        result *= 10.0f64.powi(exponent_part * exponent_sign);
    }

    result
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_bsearch(
    key: *const c_void,
    base: *const c_void,
    nel: c_size_t,
    width: c_size_t,
    cmp: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) -> *mut c_void {
    if key.is_null() || base.is_null() || nel == 0 || width == 0 || cmp.is_none() {
        return ptr::null_mut();
    }

    let cmp_fn = cmp.unwrap();
    let mut low: c_size_t = 0;
    let mut high = nel - 1;

    while low <= high {
        let mid = low + (high - low) / 2;
        let elem = base.add(mid * width);
        let comparison = cmp_fn(key, elem);

        match comparison.cmp(&0) {
            Ordering::Equal => return elem as *mut c_void,
            Ordering::Less => {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
            Ordering::Greater => low = mid + 1,
        }
    }

    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_qsort(
    base: *mut c_void,
    nel: c_size_t,
    width: c_size_t,
    cmp: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) {
    if base.is_null() || nel <= 1 || width == 0 || cmp.is_none() {
        return;
    }

    let cmp_fn = cmp.unwrap();

    unsafe fn quicksort(
        base: *mut c_void,
        low: c_size_t,
        high: c_size_t,
        width: c_size_t,
        cmp_fn: unsafe extern "C" fn(*const c_void, *const c_void) -> c_int,
    ) {
        if low < high {
            let pi = partition(base, low, high, width, cmp_fn);
            if pi > 0 {
                quicksort(base, low, pi - 1, width, cmp_fn);
            }
            quicksort(base, pi + 1, high, width, cmp_fn);
        }
    }

    unsafe fn partition(
        base: *mut c_void,
        low: c_size_t,
        high: c_size_t,
        width: c_size_t,
        cmp_fn: unsafe extern "C" fn(*const c_void, *const c_void) -> c_int,
    ) -> c_size_t {
        let pivot = base.add(high * width);
        let mut i = low;

        for j in low..high {
            let elem_j = base.add(j * width);
            if cmp_fn(elem_j, pivot) <= 0 {
                let elem_i = base.add(i * width);
                swap_bytes(elem_i, elem_j, width);
                i += 1;
            }
        }

        let elem_i = base.add(i * width);
        let elem_high = base.add(high * width);
        swap_bytes(elem_i, elem_high, width);

        i
    }

    unsafe fn swap_bytes(a: *mut c_void, b: *mut c_void, width: c_size_t) {
        let a = a as *mut u8;
        let b = b as *mut u8;

        for i in 0..width {
            let temp = *a.add(i);
            *a.add(i) = *b.add(i);
            *b.add(i) = temp;
        }
    }

    quicksort(base, 0, nel - 1, width, cmp_fn);
}

#[no_mangle]
pub unsafe extern "C" fn rust_sqlite_wasm_vec_fpclassifyl(x: c_double) -> c_int {
    if x.is_nan() {
        0
    } else if x.is_infinite() {
        if x.is_sign_positive() {
            1
        } else {
            2
        }
    } else if x == 0.0 {
        if x.is_sign_positive() {
            3
        } else {
            4
        }
    } else {
        5
    }
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;
    use std::ffi::CString;

    unsafe fn test_atoi(input: &str) -> c_int {
        let c_string = CString::new(input).unwrap();
        rust_sqlite_wasm_vec_atoi(c_string.as_ptr())
    }

    unsafe fn test_strtol(input: &str, base: c_int) -> (c_long, String) {
        let c_string = CString::new(input).unwrap();
        let mut endptr: *mut c_char = ptr::null_mut();
        let result = rust_sqlite_wasm_vec_strtol(c_string.as_ptr(), &mut endptr, base);
        let remaining = if endptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(endptr)
                .to_string_lossy()
                .into_owned()
        };
        (result, remaining)
    }

    unsafe fn test_strtod(input: &str) -> (c_double, String) {
        let c_string = CString::new(input).unwrap();
        let mut endptr: *mut c_char = ptr::null_mut();
        let result = rust_sqlite_wasm_vec_strtod(c_string.as_ptr(), &mut endptr);
        let remaining = if endptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(endptr)
                .to_string_lossy()
                .into_owned()
        };
        (result, remaining)
    }

    unsafe extern "C" fn int_compare(a: *const c_void, b: *const c_void) -> c_int {
        let a = *(a as *const c_int);
        let b = *(b as *const c_int);
        a.cmp(&b) as c_int
    }

    #[wasm_bindgen_test]
    fn test_atoi_comprehensive() {
        unsafe {
            assert_eq!(test_atoi("0"), 0);
            assert_eq!(test_atoi("123"), 123);
            assert_eq!(test_atoi("+456"), 456);
            assert_eq!(test_atoi("-789"), -789);
            assert_eq!(test_atoi("   123"), 123);
            assert_eq!(test_atoi("   -456"), -456);
            assert_eq!(test_atoi("123abc"), 123);
            assert_eq!(test_atoi(""), 0);
            assert_eq!(test_atoi("abc"), 0);
            assert_eq!(test_atoi("2147483647"), c_int::MAX);
            assert_eq!(test_atoi("-2147483648"), c_int::MIN);
        }
    }

    #[wasm_bindgen_test]
    fn test_strtol_basic() {
        unsafe {
            assert_eq!(test_strtol("123", 10), (123, "".to_string()));
            assert_eq!(test_strtol("-456", 10), (-456, "".to_string()));
            assert_eq!(test_strtol("  789", 10), (789, "".to_string()));
        }
    }

    #[wasm_bindgen_test]
    fn test_strtol_base() {
        unsafe {
            assert_eq!(test_strtol("1010", 2), (10, "".to_string()));
            assert_eq!(test_strtol("ff", 16), (255, "".to_string()));
            assert_eq!(test_strtol("077", 8), (63, "".to_string()));
            assert_eq!(test_strtol("0xff", 0), (255, "".to_string()));
            assert_eq!(test_strtol("077", 0), (63, "".to_string()));
        }
    }

    #[wasm_bindgen_test]
    fn test_strtol_endptr() {
        unsafe {
            assert_eq!(test_strtol("123abc", 10), (123, "abc".to_string()));
            assert_eq!(test_strtol("456.789", 10), (456, ".789".to_string()));
            assert_eq!(test_strtol("abc123", 10), (0, "abc123".to_string()));
        }
    }

    #[wasm_bindgen_test]
    fn test_strtod_basic() {
        unsafe {
            let (result, remaining) = test_strtod("123.45");
            assert!((result - 123.45).abs() < 1e-10);
            assert_eq!(remaining, "");

            let (result, remaining) = test_strtod("-67.89");
            assert!((result - (-67.89)).abs() < 1e-10);
            assert_eq!(remaining, "");
        }
    }

    #[wasm_bindgen_test]
    fn test_strtod_scientific() {
        unsafe {
            let (result, remaining) = test_strtod("1.23e2");
            assert!((result - 123.0).abs() < 1e-10);
            assert_eq!(remaining, "");

            let (result, remaining) = test_strtod("4.56e-2");
            assert!((result - 0.0456).abs() < 1e-10);
            assert_eq!(remaining, "");
        }
    }

    #[wasm_bindgen_test]
    fn test_strtod_endptr() {
        unsafe {
            let (result, remaining) = test_strtod("123.45abc");
            assert!((result - 123.45).abs() < 1e-10);
            assert_eq!(remaining, "abc");

            let (result, remaining) = test_strtod("abc123");
            assert_eq!(result, 0.0);
            assert_eq!(remaining, "abc123");
        }
    }

    #[wasm_bindgen_test]
    fn test_bsearch() {
        unsafe {
            let array = [1, 3, 5, 7, 9];
            let key = 5;
            let result = rust_sqlite_wasm_vec_bsearch(
                &key as *const _ as *const _,
                array.as_ptr() as *const _,
                array.len() as c_size_t,
                std::mem::size_of::<c_int>() as c_size_t,
                Some(int_compare),
            );
            assert!(!result.is_null());
            assert_eq!(*(result as *const c_int), 5);

            let key_not_found = 4;
            let result = rust_sqlite_wasm_vec_bsearch(
                &key_not_found as *const _ as *const _,
                array.as_ptr() as *const _,
                array.len() as c_size_t,
                std::mem::size_of::<c_int>() as c_size_t,
                Some(int_compare),
            );
            assert!(result.is_null());
        }
    }

    #[wasm_bindgen_test]
    fn test_qsort() {
        unsafe {
            let mut array = [9, 3, 7, 1, 5];
            let expected = [1, 3, 5, 7, 9];

            rust_sqlite_wasm_vec_qsort(
                array.as_mut_ptr() as *mut _,
                array.len() as c_size_t,
                std::mem::size_of::<c_int>() as c_size_t,
                Some(int_compare),
            );

            assert_eq!(array, expected);
        }
    }

    #[wasm_bindgen_test]
    fn test_fpclassifyl() {
        unsafe {
            assert_eq!(rust_sqlite_wasm_vec_fpclassifyl(f64::NAN), 0);
            assert_eq!(rust_sqlite_wasm_vec_fpclassifyl(f64::INFINITY), 1);
            assert_eq!(rust_sqlite_wasm_vec_fpclassifyl(f64::NEG_INFINITY), 2);
            assert_eq!(rust_sqlite_wasm_vec_fpclassifyl(0.0), 3);
            assert_eq!(rust_sqlite_wasm_vec_fpclassifyl(-0.0), 4);
            assert_eq!(rust_sqlite_wasm_vec_fpclassifyl(123.45), 5);
        }
    }

    #[wasm_bindgen_test]
    fn test_null_handling() {
        unsafe {
            assert_eq!(rust_sqlite_wasm_vec_atoi(ptr::null()), 0);

            let mut endptr: *mut c_char = ptr::null_mut();
            assert_eq!(rust_sqlite_wasm_vec_strtol(ptr::null(), &mut endptr, 10), 0);
            assert_eq!(rust_sqlite_wasm_vec_strtod(ptr::null(), &mut endptr), 0.0);

            assert!(rust_sqlite_wasm_vec_bsearch(ptr::null(), ptr::null(), 0, 0, None).is_null());

            rust_sqlite_wasm_vec_qsort(ptr::null_mut(), 0, 0, None);
        }
    }
}
