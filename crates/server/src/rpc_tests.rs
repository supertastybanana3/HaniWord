use super::*;
use hyper_util::rt::TokioIo;
use shared::proto::azookey_service_client::AzookeyServiceClient;
use tokio::net::windows::named_pipe::ClientOptions;
use tonic::{transport::{Channel, Endpoint}, Code};
use tower::service_fn;
use std::os::windows::io::IntoRawHandle;

async fn client(base: &str) -> AzookeyServiceClient<Channel> {
    let path = shared::winsec::pipe_name(base);
    client_path(&path).await
}

async fn client_path(path: &str) -> AzookeyServiceClient<Channel> {
    let path = path.to_owned();
    let channel = Endpoint::from_static("http://localhost:50051")
        .connect_with_connector(service_fn(move |_| {
            let path = path.clone();
            async move {
                for _ in 0..100 {
                    match shared::winsec::open_client_pipe(&path, "azookey-server.exe") {
                        Ok(handle) => return unsafe { tokio::net::windows::named_pipe::NamedPipeClient::from_raw_handle(handle.into_raw_handle()) }.map(TokioIo::new),
                        Err(error) if error.raw_os_error() == Some(231) => {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Test pipe remained busy"))
            }
        })).await.unwrap();
    AzookeyServiceClient::new(channel)
}

async fn edit(client: &mut AzookeyServiceClient<Channel>, op: edit_text_request::Op, text: &str, offset: i32) -> EditTextResponse {
    client.edit_text(EditTextRequest { op: op as i32, text: text.into(), offset })
        .await.unwrap().into_inner()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_rpc_isolates_input_and_rejects_invalid_requests() {
    let Some(executable) = std::env::var_os("AZK_PROBE_SERVER") else {
        println!("RPC fixture not configured; run verify_ipc_security.ps1 for the native integration test");
        return;
    };
    let fixture = std::path::PathBuf::from(&executable).parent().unwrap().to_owned();
    assert!(fixture.join("isolated_security_fixture.json").is_file(), "RPC fixture marker required");
    let output = std::fs::File::create(fixture.join("rpc_server_stdout.txt")).unwrap();
    let errors = std::fs::File::create(fixture.join("rpc_server_stderr.txt")).unwrap();
    struct Probe(std::process::Child);
    impl Drop for Probe {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let base = format!("haniword_rpc_test_{}", std::process::id());
    let mut server = Probe(std::process::Command::new(executable).env("AZK_TEST_PIPE", &base)
        .stdout(output).stderr(errors).spawn().unwrap());
    let pipe = shared::winsec::pipe_name(&base);
    let mut ready = false;
    for _ in 0..200 {
        if let Ok(probe) = ClientOptions::new().open(&pipe) { drop(probe); ready = true; break; }
        assert!(server.0.try_wait().unwrap().is_none(), "Isolated server exited at startup");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ready, "Isolated server never opened its test-only pipe");
    let mut rejected = shared::winsec::open_client_pipe(&pipe, "not-the-ime-server.exe").unwrap_err();
    for _ in 0..100 {
        if rejected.raw_os_error() != Some(231) { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        rejected = shared::winsec::open_client_pipe(&pipe, "not-the-ime-server.exe").unwrap_err();
    }
    assert_eq!(rejected.kind(), std::io::ErrorKind::PermissionDenied);
    let mut a = client(&base).await;
    let mut b = client(&base).await;
    use edit_text_request::Op::*;
    assert_eq!(edit(&mut a, InsertDirect, "privateA", 0).await.raw, "privateA");
    assert_eq!(edit(&mut b, DeleteBackward, "", 0).await.raw, "");
    assert_eq!(edit(&mut b, InsertDirect, "privateB", 0).await.raw, "privateB");
    let moved = edit(&mut a, MoveCursor, "", -2).await;
    assert_eq!((moved.raw.as_str(), moved.cursor), ("privateA", 6));
    assert_eq!(edit(&mut b, MoveCursor, "", 0).await.raw, "privateB");
    let mut fresh = client(&base).await;
    assert!(edit(&mut fresh, MoveCursor, "", 0).await.raw.is_empty());
    edit(&mut fresh, InsertDirect, &"x".repeat(200), 0).await;
    assert_eq!(edit(&mut fresh, MoveCursor, "", -128).await.cursor, 72);
    assert_eq!(edit(&mut fresh, MoveCursor, "", 128).await.cursor, 200);
    for text in ["bad\0input".to_string(), "x".repeat(security::MAX_INPUT_CHARS + 1)] {
        let rejected = fresh.edit_text(EditTextRequest { op: InsertDirect as i32, text, offset: 0 }).await.unwrap_err();
        assert!(matches!(rejected.code(), Code::InvalidArgument | Code::ResourceExhausted));
    }
    assert_eq!(fresh.shrink_text(ShrinkTextRequest { offset: -1 }).await.unwrap_err().code(), Code::InvalidArgument);
    assert_eq!(fresh.set_context(SetContextRequest { context: "bad\0context".into(), ..Default::default() }).await.unwrap_err().code(), Code::InvalidArgument);
    // 2026-11-08 (仕事 45): カーソルの右の字も同じに確かめる
    assert_eq!(fresh.set_context(SetContextRequest { context: "前の文".into(), right_context: "bad\0right".into() }).await.unwrap_err().code(), Code::InvalidArgument);
    assert!(fresh.set_context(SetContextRequest { context: "前の文".into(), right_context: "、次の字\r\n次の行".into() }).await.is_ok());
    assert_eq!(fresh.list_candidates(ListCandidatesRequest { hiragana: "bad\0reading".into(), ..Default::default() }).await.unwrap_err().code(), Code::InvalidArgument);
    assert_eq!(fresh.learn(LearnRequest { readings: vec!["a".into()], texts: vec![], context: String::new() }).await.unwrap_err().code(), Code::InvalidArgument);
    assert_eq!(edit(&mut a, MoveCursor, "", 0).await.raw, "privateA");

    // A real restricted token may convert but may not write learning/configuration.
    let restricted_base = base.clone();
    let restricted = std::thread::spawn(move || {
        use windows::Win32::{Foundation::{HANDLE, HLOCAL, LocalFree}, Security::{CreateRestrictedToken, DISABLE_MAX_PRIVILEGE,
            Authorization::ConvertStringSidToSidW, PSID, SID_AND_ATTRIBUTES,
            ImpersonateLoggedOnUser, RevertToSelf, TOKEN_DUPLICATE, TOKEN_QUERY},
            System::Threading::{GetCurrentProcess, OpenProcessToken}};
        let pipe_path = shared::winsec::pipe_name(&restricted_base);
        let mut raw = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut raw).unwrap(); }
        let original = shared::winsec::OwnedHandle(raw);
        let mut raw = HANDLE::default();
        let mut sid = PSID::default();
        unsafe { ConvertStringSidToSidW(windows::core::w!("S-1-5-12"), &mut sid).unwrap(); }
        let restricting = [SID_AND_ATTRIBUTES { Sid: sid, Attributes: 0 }];
        let created = unsafe { CreateRestrictedToken(original.0, DISABLE_MAX_PRIVILEGE, None, None, Some(&restricting), &mut raw) };
        unsafe { let _ = LocalFree(HLOCAL(sid.0)); }
        created.unwrap();
        let token = shared::winsec::OwnedHandle(raw);
        unsafe { ImpersonateLoggedOnUser(token.0).unwrap(); }
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            assert!(!shared::winsec::private_data_allowed());
            let config = shared::AppConfig::read_or_default();
            assert!(config.zenzai.profile.is_empty());
            assert_eq!(config.input.latin_languages, ["es"]);
            assert!(!config.input.english_case_fix);
            let denied = tokio::net::windows::named_pipe::ServerOptions::new().create(&pipe_path).unwrap_err();
            assert_eq!(denied.raw_os_error(), Some(5), "Sandbox must not create another server instance");
            {
                use std::os::windows::io::AsRawHandle;
                use windows::Win32::{System::{Pipes::GetNamedPipeServerProcessId,
                    Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ}}, Security::TOKEN_ACCESS_MASK};
                let pipe = shared::winsec::open_client_pipe(&pipe_path, "azookey-server.exe").unwrap();
                let mut pid = 0;
                unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid).unwrap(); }
                assert!(unsafe { OpenProcess(PROCESS_VM_READ, false, pid) }.is_err(), "Identity query must not grant memory reads");
                let process = shared::winsec::OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).unwrap() });
                let mut duplicate = HANDLE::default();
                assert!(unsafe { OpenProcessToken(process.0, TOKEN_ACCESS_MASK(0x000a), &mut duplicate) }.is_err(),
                    "Identity query must not grant token duplication");
            }
            let mut restricted = client_path(&pipe_path).await;
            assert_eq!(edit(&mut restricted, InsertDirect, "restricted", 0).await.raw, "restricted");
            assert_eq!(restricted.learn(LearnRequest { readings: vec!["つじ".into()], texts: vec!["辻".into()], context: String::new() })
                .await.unwrap_err().code(), Code::PermissionDenied);
            assert_eq!(restricted.update_config(UpdateConfigRequest {}).await.unwrap_err().code(), Code::PermissionDenied);
        });
        unsafe { RevertToSelf().unwrap(); }
    });
    tokio::task::spawn_blocking(move || restricted.join().unwrap()).await.unwrap();
    drop((a, b, fresh));
    println!("Isolated RPC: authenticated normal/restricted tokens; per-connection input; invalid input rejected");
}
