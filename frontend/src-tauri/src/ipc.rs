use anyhow::Result;
use hyper_util::rt::TokioIo;
use shared::proto::azookey_service_client::AzookeyServiceClient;
use std::{sync::Arc, time::Duration};
use std::os::windows::io::IntoRawHandle;
use tokio::{net::windows::named_pipe::NamedPipeClient, time};
use tonic::transport::Endpoint;
use tower::service_fn;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

fn open_pipe() -> std::io::Result<NamedPipeClient> {
    let open = |path: &str| {
        let handle = shared::winsec::open_client_pipe(path, "azookey-server.exe")?;
        unsafe { NamedPipeClient::from_raw_handle(handle.into_raw_handle()) }
    };
    match open(&shared::winsec::pipe_name("haniword_server")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => open(&shared::winsec::legacy_pipe_name("haniword_server")),
        other => other,
    }
}

// connect to kkc server
#[derive(Debug, Clone)]
pub struct IPCService {
    // kkc server client
    azookey_client: AzookeyServiceClient<tonic::transport::channel::Channel>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl IPCService {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;

        // Merely opening settings must not connect to or mutate the live IME.
        let server_channel = {
            let _entered = runtime.enter();
            Endpoint::try_from("http://[::]:50051")?.connect_with_connector_lazy(service_fn(
                |_| async {
                    let client = loop {
                        match open_pipe() {
                            Ok(client) => break client,
                            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => (),
                            Err(e) => return Err(e),
                        }

                        time::sleep(Duration::from_millis(50)).await;
                    };

                    Ok::<_, std::io::Error>(TokioIo::new(client))
                },
            ))
        };

        let azookey_client = AzookeyServiceClient::new(server_channel);

        Ok(Self {
            azookey_client,
            runtime: Arc::new(runtime),
        })
    }
}

// implement methods to interact with kkc server
impl IPCService {
    pub fn update_config(&mut self) -> anyhow::Result<()> {
        let request = tonic::Request::new(shared::proto::UpdateConfigRequest {});
        self.runtime
            .clone()
            .block_on(self.azookey_client.update_config(request))?;

        Ok(())
    }

    /// 2026-11-08: 学習をまっさらにする (変換サーバーを通して消す)。include_tables が真なら HaniWord の学習の表も。控えの場所を返す
    pub fn reset_learning(&mut self, include_tables: bool) -> anyhow::Result<String> {
        let request = tonic::Request::new(shared::proto::ResetLearningRequest { include_tables });
        let response = self
            .runtime
            .clone()
            .block_on(self.azookey_client.reset_learning(request))?;
        Ok(response.into_inner().backup)
    }
}
