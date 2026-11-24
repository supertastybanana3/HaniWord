// 1 文字ずつ打ったときのサーバーの読みと「打った文字の列」を見る
use hyper_util::rt::TokioIo;
use shared::proto::{azookey_service_client::AzookeyServiceClient, edit_text_request::Op};
use tokio::net::windows::named_pipe::ClientOptions;
use tonic::transport::Endpoint;
use tower::service_fn;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let channel = Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(|_| async {
            let client = ClientOptions::new().open(r"\\.\pipe\haniword_server")?;
            Ok::<_, std::io::Error>(TokioIo::new(client))
        }))
        .await?;
    let mut c = AzookeyServiceClient::new(channel);
    let mut out = String::new();
    for word in ["windows", "Windows"] {
        c.clear_text(shared::proto::ClearTextRequest {}).await?;
        for ch in word.chars() {
            let r = c.edit_text(shared::proto::EditTextRequest { op: Op::Insert as i32, text: ch.to_string(), offset: 0 }).await?.into_inner();
            out += &format!("{ch}: hiragana={} raw={}\n", r.hiragana, r.raw);
        }
        out += "---\n";
    }
    c.clear_text(shared::proto::ClearTextRequest {}).await?;
    std::fs::write(std::env::temp_dir().join("smoke_raw.txt"), out)?;
    Ok(())
}
