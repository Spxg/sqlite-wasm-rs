use crate::c::*;
use crate::libsqlite3::*;
use std::sync::mpsc::Sender;

pub struct Req {
    pub payload: Request,
    pub tx: Sender<Response>,
}

#[allow(non_camel_case_types)]
pub enum Request {
    sqlite3_open_v2(
        (
            *const ::std::os::raw::c_char,
            *mut *mut sqlite3,
            ::std::os::raw::c_int,
            *const ::std::os::raw::c_char,
        ),
    ),
}

impl Req {
    pub(crate) fn call(self) {
        match self.payload {
            Request::sqlite3_open_v2((a, b, c, d)) => {
                let resp = unsafe { sqlite3_open_v2(a, b, c, d) };
                self.tx.send(Response::sqlite3_open_v2(resp)).unwrap();
            }
        }
    }
}

unsafe impl Send for Request {}
unsafe impl Sync for Request {}

#[allow(non_camel_case_types)]
pub enum Response {
    sqlite3_open_v2(::std::os::raw::c_int),
}

unsafe impl Send for Response {}
unsafe impl Sync for Response {}
