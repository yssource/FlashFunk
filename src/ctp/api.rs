#![allow(dead_code, unused_variables, unused_imports)]


use super::interface::Interface;
use std::ffi::{CStr, CString};
use std::os::raw::{c_void, c_char, c_int, c_uchar};
use crate::ctp::sys::{CThostFtdcMdApi, CThostFtdcTraderApi, CThostFtdcMdApi_Init,
                      CThostFtdcMdApi_RegisterFront, CThostFtdcMdApi_SubscribeMarketData,
                      QuoteSpi, CThostFtdcMdApi_GetTradingDay, CThostFtdcMdApi_CreateFtdcMdApi,
                      CThostFtdcReqUserLoginField, CThostFtdcUserLogoutField, CThostFtdcFensUserInfoField,
                      CThostFtdcSpecificInstrumentField, CThostFtdcRspInfoField, CThostFtdcDepthMarketDataField,
                      CThostFtdcForQuoteRspField, CThostFtdcRspUserLoginField, TThostFtdcRequestIDType,
                      TThostFtdcErrorIDType};
use std::process::id;
use actix::Addr;
use crate::app::CtpbeeR;
use std::fmt;
use std::borrow::{Cow, BorrowMut};

use encoding::{DecoderTrap, Encoding};
use encoding::all::GB18030;
use failure::_core::str::Utf8Error;
use crate::structs::{OrderRequest, CancelRequest, LoginForm};
use crate::ctp::func::QuoteApi;

#[allow(non_camel_case_types)]
type c_bool = std::os::raw::c_uchar;


/// the implement of market api
/// user_id 用户名
/// password 密码
/// path 流文件存放地址
/// market_api 行情API 收
/// market_spi 行情API 回调
pub struct MdApi {
    user_id: CString,
    password: CString,
    path: CString,
    market_api: *mut CThostFtdcMdApi,
    market_spi: Option<*mut QuoteSpi>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectionReason {
    ReadError = 0x1001,
    WriteError = 0x1002,
    HeartbeatTimeout = 0x2001,
    HeartbeatSendError = 0x2002,
    ErrorMessageReceived = 0x2003,
    Unknown = 0x0000,
}

impl std::convert::From<c_int> for DisconnectionReason {
    fn from(reason: c_int) -> DisconnectionReason {
        match reason {
            0x1001 => DisconnectionReason::ReadError,
            0x1002 => DisconnectionReason::WriteError,
            0x2001 => DisconnectionReason::HeartbeatTimeout,
            0x2002 => DisconnectionReason::HeartbeatSendError,
            0x2003 => DisconnectionReason::ErrorMessageReceived,
            _ => DisconnectionReason::Unknown,
        }
    }
}

#[must_use]
pub type RspResult = Result<(), RspError>;

#[derive(Clone, Debug, PartialEq)]
pub struct RspError {
    pub id: TThostFtdcErrorIDType,
    pub msg: String,
}

impl std::error::Error for RspError {}

impl fmt::Display for RspError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.id, self.msg)
    }
}


pub fn result_to_string(rsp_result: &RspResult) -> String {
    match rsp_result {
        Ok(()) => "Ok(())".to_string(),
        Err(err) => format!("Err(RspError{{ id: {}, msg: {} }})", err.id, err.msg),
    }
}

pub unsafe fn info_to_result(rsp_info: *const CThostFtdcRspInfoField) -> RspResult {
    #[allow(unused_unsafe)] // for future "unsafe blocks in unsafe fn" feature
    match unsafe { rsp_info.as_ref() } {
        Some(info) => match info {
            CThostFtdcRspInfoField { ErrorID: 0, .. } => {
                Ok(())
            }
            CThostFtdcRspInfoField { ErrorID: id, ErrorMsg: msg } => {
                Err(RspError { id: *id, msg: covert_cstr_to_str(msg).into_owned() })
            }
        },
        None => {
            Ok(())
        }
    }
}

// pub extern fn covert_to_str(to: *const c_char) -> String {
//     let c_str = unsafe { CString::fr(to) };
//     c_str.to_str().unwrap().to_string()
// }
/// todo: 下面有问题描述
pub fn covert_cstr_to_str(v: &[i8]) -> Cow<str> {
    Cow::from("这里有严重的问题， 我不知道怎么把i8的c_char转换为 String")
}


// fn create_spi(md_spi: *mut dyn QuoteApi, addr: Addr<CtpbeeR>) -> CThostFtdcMdSpi {
//     CThostFtdcMdSpi { vtable: &SPI_VTABLE, spi: md_spi, addr }
// }


/// Now we get a very useful spi, and we get use the most important things to let everything works well
/// the code is from ctp-rs
///
/// 实现行情API的一些主动基准调用方法
impl MdApi {
    pub fn new(id: String, pwd: String, path: String) -> MdApi {
        let ids = CString::new(id).unwrap();
        let pwds = CString::new(pwd).unwrap();
        let paths = CString::new(path).unwrap();
        let flow_path_ptr = paths.as_ptr();
        let api = unsafe { CThostFtdcMdApi_CreateFtdcMdApi(flow_path_ptr, true, true) };
        MdApi {
            user_id: ids,
            password: pwds,
            path: paths,
            market_api: api,
            market_spi: None,
        }
    }
    /// 初始化调用
    pub fn init(&mut self) -> bool {
        unsafe { CThostFtdcMdApi_Init(self.market_api) };
        true
    }
    /// 获取交易日
    pub fn get_trading_day<'a>(&mut self) -> &'a str {
        let trading_day_cstr = unsafe { CThostFtdcMdApi_GetTradingDay(self.market_api) };
        unsafe { CStr::from_ptr(trading_day_cstr as *const i8).to_str().unwrap() }
    }

    /// 注册前置地址
    fn register_fronted_address(&mut self, register_addr: CString) {
        let front_socket_address_ptr = register_addr.into_raw();
        unsafe { CThostFtdcMdApi_RegisterFront(self.market_api, front_socket_address_ptr) };
    }

    /// 注册回调
    fn register_spi(&mut self, quo_api: Box<dyn QuoteApi>, addr: Addr<CtpbeeR>) {
        //解开引用
        let last_registered_spi_ptr = self.market_spi.take();
        // 获取回调操作结构体
        let md_spi_ptr = Box::into_raw(quo_api);
        // // 创建我们需要的回调结构体
        // let spi_ptr = Box::into_raw(Box::new(create_spi(md_spi_ptr, addr)));
        // unsafe { CThostFtdcMdApi_RegisterSpi(self.market_api, spi_ptr.) };
        // // // 更新到本地的结构体,注册0成功
        // self.market_spi = Some(spi_ptr);
    }
}

impl Interface for MdApi {
    fn send_order(&mut self, order: OrderRequest) -> String {
        unimplemented!("行情接口无此功能")
    }

    fn cancel_order(&mut self, req: CancelRequest) {
        unimplemented!("行情接口无此功能")
    }

    fn connect(&mut self, req: LoginForm) {}

    fn subscribe(&mut self, symbol: String) {
        let code = CString::new(symbol).unwrap();
        let mut c = code.into_raw();
        unsafe { CThostFtdcMdApi_SubscribeMarketData(self.market_api, c.borrow_mut(), 1) };
    }

    fn unsubscribe(&mut self, symbol: String) {
        unimplemented!()
    }

    fn exit(&mut self) {
        unimplemented!()
    }

    fn get_app(&mut self) -> &Addr<CtpbeeR> {
        unimplemented!()
    }
}