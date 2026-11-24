use async_stream::stream;
use futures_core::stream::Stream;
use std::{ffi::c_void, pin::Pin, ptr::addr_of_mut, os::windows::io::AsRawHandle,
    sync::{Arc, OnceLock, atomic::{AtomicU64, Ordering}}};
use tokio::{
    io::{self, AsyncRead, AsyncWrite},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::{Semaphore, OwnedSemaphorePermit},
};
use tonic::transport::server::Connected;
use windows::{
    Win32::Foundation::{HANDLE, HLOCAL, LocalFree},
    Win32::Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
};

#[allow(dead_code)]
struct UnsafeSecurityAttributes(SECURITY_ATTRIBUTES);

unsafe impl Send for UnsafeSecurityAttributes {}
unsafe impl Sync for UnsafeSecurityAttributes {}

impl Drop for UnsafeSecurityAttributes {
    fn drop(&mut self) { unsafe { let _ = LocalFree(HLOCAL(self.0.lpSecurityDescriptor)); } }
}

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);
pub const MAX_CONNECTIONS: usize = 128;

#[derive(Clone)]
pub struct ConnectionInfo(pub Arc<ConnectionState>);

pub struct ConnectionState {
    pub id: u64,
    pub peer: OnceLock<shared::winsec::PipePeer>,
    /// 2026-11-23 (点検 Astra 02): 相手の処理の番号。最初の読み取りで、相手を確かめるのと同じ所で入れる (取れなければ空のまま)
    pub pid: OnceLock<u32>,
    _permit: OwnedSemaphorePermit,
}

pub struct TonicNamedPipeServer {
    inner: NamedPipeServer,
    info: ConnectionInfo,
}

impl Connected for TonicNamedPipeServer {
    type ConnectInfo = ConnectionInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        self.info.clone()
    }
}

impl AsyncRead for TonicNamedPipeServer {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(result, std::task::Poll::Ready(Ok(()))) && buf.filled().len() > before && self.info.0.peer.get().is_none() {
            let handle = HANDLE(self.inner.as_raw_handle());
            match shared::winsec::authenticate_pipe_client(handle) {
                Ok(peer) => {
                    if let Some(pid) = shared::winsec::pipe_client_pid(handle) { let _ = self.info.0.pid.set(pid); }
                    let _ = self.info.0.peer.set(peer);
                }
                Err(error) => return std::task::Poll::Ready(Err(error)),
            }
        }
        result
    }
}

impl AsyncWrite for TonicNamedPipeServer {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn create_pipe(name: &str, first: bool, attributes: &mut UnsafeSecurityAttributes) -> io::Result<NamedPipeServer> {
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, addr_of_mut!(attributes.0) as *mut c_void)
    }
}

fn pipe_attributes(sddl: &str) -> io::Result<UnsafeSecurityAttributes> {
    let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            windows::core::PCWSTR(sddl.as_ptr()), SDDL_REVISION, &mut descriptor, None,
        ).map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error.to_string()))?;
    }
    Ok(UnsafeSecurityAttributes(SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    }))
}

impl TonicNamedPipeServer {
    /// 通信路を待ち受ける。名前は利用者 (セッション) ごと (`<path>_s<番号>`) と、移行のあいだだけ以前の名前 (`<path>`) の 2 つ。
    /// 開けるのは 自分・隔離されたアプリ・システムだけ (shared::winsec::pipe_sddl)。ネットワーク越しの接続は断る
    pub fn new(path: &str) -> impl Stream<Item = io::Result<TonicNamedPipeServer>> {
        // 隔離されたアプリ (ストアアプリ等) の中の IME からもつなげるようにする
        // see https://nathancorvussolis.blogspot.com/2018/05/windows-ime-security.html
        let names = [shared::winsec::pipe_name(path), shared::winsec::legacy_pipe_name(path)];
        let sddl_result = shared::winsec::pipe_sddl();
        let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));

        stream! {
            let mut security_attributes = pipe_attributes(&sddl_result?)?;

                let mut primary = create_pipe(&names[0], true, &mut security_attributes)?;
                // 以前の名前は、別のサーバーがまだ使っていれば待ち受けない
                let mut legacy = create_pipe(&names[1], true, &mut security_attributes).ok();

                loop {
                    let connected: io::Result<bool> = match legacy.as_ref() {
                        Some(server) => tokio::select! {
                            result = primary.connect() => result.map(|_| false),
                            result = server.connect() => result.map(|_| true),
                        },
                        None => primary.connect().await.map(|_| false),
                    };
                    let use_legacy = connected?;
                    let connected = if use_legacy {
                        let next = create_pipe(&names[1], false, &mut security_attributes)?;
                        legacy.replace(next).unwrap()
                    } else {
                        let next = create_pipe(&names[0], false, &mut security_attributes)?;
                        std::mem::replace(&mut primary, next)
                    };
                    // Close excess connections instead of allocating unbounded transport/session state.
                    let Ok(permit) = slots.clone().try_acquire_owned() else { continue; };
                    let info = ConnectionInfo(Arc::new(ConnectionState {
                        id: NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed),
                        peer: OnceLock::new(), pid: OnceLock::new(), _permit: permit,
                    }));
                    yield Ok(TonicNamedPipeServer { inner: connected, info });
                }
        }
    }
}
