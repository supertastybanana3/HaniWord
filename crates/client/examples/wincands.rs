// Windows 標準の変換候補 API (Windows.Data.Text.TextConversionGenerator) が返す候補を確かめる
use std::time::Instant;
use windows::{core::HSTRING, Data::Text::TextConversionGenerator, Foundation::AsyncStatus};

fn main() -> windows::core::Result<()> {
    use std::io::Write;
    let mut out = std::fs::File::create(std::env::temp_dir().join("wincands.txt")).unwrap();
    macro_rules! println { ($($t:tt)*) => { writeln!(out, $($t)*).unwrap() } }
    let t = Instant::now();
    let generator = TextConversionGenerator::Create(&HSTRING::from("ja"))?;
    println!("create {:?} resolved={} notInstalled={}", t.elapsed(), generator.ResolvedLanguage()?, generator.LanguageAvailableButNotInstalled()?);
    for reading in std::env::var("AZK_READINGS").unwrap_or("ゆり".into()).split(",").map(String::from).collect::<Vec<_>>().iter().map(|s| s.as_str()) {
        let t = Instant::now();
        let op = generator.GetCandidatesWithMaxCountAsync(&HSTRING::from(reading), 100)?;
        while op.Status()? == AsyncStatus::Started {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let list = op.GetResults()?;
        let items: Vec<String> = (0..list.Size()?).map(|i| list.GetAt(i).map(|s| s.to_string()).unwrap_or_default()).collect();
        println!("{reading}\t{}個\t{:?}\t{}", items.len(), t.elapsed(), items.iter().take(40).cloned().collect::<Vec<_>>().join(" "));
    }
    Ok(())
}
