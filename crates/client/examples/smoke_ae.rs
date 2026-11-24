// 英語の自動判別で使うサーバーのやり取りを試す
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
    c.clear_text(shared::proto::ClearTextRequest {}).await?;
    let mut last = shared::proto::EditTextResponse::default();
    for (op, text) in [(Op::Insert, "kyouha"), (Op::InsertDirect, "google"), (Op::Insert, "dekensaku")] {
        last = c.edit_text(shared::proto::EditTextRequest { op: op as i32, text: text.into(), offset: 0 }).await?.into_inner();
        out += &format!("{op:?} {text} -> hiragana={} raw={}\n", last.hiragana, last.raw);
    }
    let r = c.convert(shared::proto::ConvertRequest { hiragana: last.hiragana.clone(), context: String::new(), fast: false, dictionary_only: false }).await?.into_inner();
    out += &format!("convert: {}\n", r.segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join(" / "));
    c.clear_text(shared::proto::ClearTextRequest {}).await?;
    for (op, text) in [(Op::Insert, "kore"), (Op::InsertDirect, "python"), (Op::Insert, "desu")] {
        last = c.edit_text(shared::proto::EditTextRequest { op: op as i32, text: text.into(), offset: 0 }).await?.into_inner();
    }
    let r = c.convert(shared::proto::ConvertRequest { hiragana: last.hiragana.clone(), context: String::new(), fast: false, dictionary_only: false }).await?.into_inner();
    out += &format!("{} -> {}\n", last.hiragana, r.segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join(" / "));
    c.clear_text(shared::proto::ClearTextRequest {}).await?;
    std::fs::write(std::env::temp_dir().join("smoke_ae.txt"), out)?;
    Ok(())
}
