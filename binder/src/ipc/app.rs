// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use serde::{Deserialize, Serialize};

use sgx_types::error::SgxStatus;
use sgx_types::types::EnclaveId;

use crate::ipc::IpcError;
use crate::ipc::IpcSender;
use log::{debug, error};
use teaclave_types::ECallStatus;

// Delaration of ecall for App, the implementation is in TEE
// This function is automatically generated by the procedure macro #[ecall_entry_point].
// fn enclave_init(eid: EnclaveId, retval: *mut ECallStatus) -> SgxStatus;

extern "C" {
    fn ecall_ipc_entry_point(
        eid: EnclaveId,
        retval: *mut ECallStatus,
        cmd: u32,
        in_buf: *const u8,
        in_len: usize,
        out_buf: *mut u8,
        out_max: usize,
        out_len: &mut usize,
    ) -> SgxStatus;
}

// Implementation of IPC Sender For App
// ECallChannel, receiver is implemented in TEE
pub struct ECallChannel {
    enclave_id: EnclaveId,
    curr_out_buf_size: usize,
}

impl ECallChannel {
    pub fn new(enclave_id: EnclaveId) -> ECallChannel {
        ECallChannel {
            enclave_id,
            curr_out_buf_size: 256,
        }
    }

    fn ecall_ipc_app_to_tee(
        &mut self,
        cmd: u32,
        request_payload: Vec<u8>,
    ) -> std::result::Result<Vec<u8>, IpcError> {
        debug! {"ecall_ipc_app_to_tee: {:x}, {:x} bytes", cmd, request_payload.len()};

        let in_ptr: *const u8 = request_payload.as_ptr();
        let in_len: usize = request_payload.len();

        let mut retried = false;
        let out_buf = loop {
            let out_max: usize = self.curr_out_buf_size;
            let mut out_buf: Vec<u8> = Vec::with_capacity(out_max);
            let mut out_len: usize = out_max;
            let out_ptr: *mut u8 = out_buf.as_mut_ptr();

            let mut ecall_ret = ECallStatus::default();

            let sgx_status = unsafe {
                ecall_ipc_entry_point(
                    self.enclave_id,
                    &mut ecall_ret,
                    cmd,
                    in_ptr,
                    in_len,
                    out_ptr,
                    out_max,
                    &mut out_len,
                )
            };

            // Check sgx return values
            if sgx_status != SgxStatus::Success {
                error!("ecall_ipc_entry_point, app sgx_error:{}", sgx_status);
                return Err(IpcError::SgxError(sgx_status));
            }

            // Check rust logic return values
            // If out_buf is not big enough, realloc based on the returned out_len
            // We only retry once for once invocation.
            if ecall_ret.is_err_ffi_outbuf() && !retried {
                debug!(
                    "ecall_ipc_entry_point, expand app request buffer size: {:?}",
                    ecall_ret
                );

                assert!(out_len > out_max);
                self.curr_out_buf_size = out_len;
                retried = true;
                continue;
            }

            // Check rust logic return values
            // Transparent deliever the errors to outer logic.
            if ecall_ret.is_err() {
                error!("ecall_ipc_entry_point, app api_error: {:?}", ecall_ret);
                return Err(IpcError::ECallError(ecall_ret));
            }

            unsafe {
                out_buf.set_len(out_len);
            }
            debug!("ecall_ipc_entry_point OK. App Received Buf: {:?}", out_buf);

            break out_buf;
        };

        Ok(out_buf)
    }
}

impl IpcSender for ECallChannel {
    fn invoke<U, V>(&mut self, cmd: u32, input: U) -> std::result::Result<V, IpcError>
    where
        U: Serialize,
        V: for<'de> Deserialize<'de>,
    {
        let request_payload = serde_json::to_vec(&input)?;
        let result_buf = self.ecall_ipc_app_to_tee(cmd, request_payload)?;
        let response: V = serde_json::from_slice(&result_buf)?;
        Ok(response)
    }
}

#[cfg(feature = "app_unit_test")]
pub mod tests {
    use super::*;

    pub fn run_tests(eid: EnclaveId) -> bool {
        let mut ecall_ret = ECallStatus::default();
        let mut out_buf = vec![0; 128];
        let mut out_len = 0usize;
        let sgx_status = unsafe {
            ecall_ipc_entry_point(
                eid,
                &mut ecall_ret,
                0x0000_1003,      //cmd,
                std::ptr::null(), //in_ptr,
                128,              //in_len,
                out_buf.as_mut_ptr(),
                128,
                &mut out_len,
            )
        };
        assert_eq!(sgx_status, SgxStatus::Success);
        assert!(ecall_ret.is_err());

        true
    }
}
