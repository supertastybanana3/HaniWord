// Windows 標準の予測候補 API (Windows.Data.Text.TextPredictionGenerator) が返す候補を確かめる
use std::time::Instant;
use windows::{core::HSTRING, Data::Text::TextPredictionGenerator, Foundation::AsyncStatus};

fn main() -> windows::core::Result<()> {
    use std::io::Write;
    let mut out = std::fs::File::create(std::env::temp_dir().join("winpredict.txt")).unwrap();
    let generator = TextPredictionGenerator::Create(&HSTRING::from("ja"))?;
    writeln!(out, "resolved={} notInstalled={}", generator.ResolvedLanguage()?, generator.LanguageAvailableButNotInstalled()?).unwrap();
    let readings = std::env::var("AZK_READINGS").unwrap_or("よろ".into());
    for reading in readings.split(',') {
        let t = Instant::now();
        let op = generator.GetCandidatesWithMaxCountAsync(&HSTRING::from(reading), 10)?;
        while op.Status()? == AsyncStatus::Started {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let list = op.GetResults()?;
        let items: Vec<String> = (0..list.Size()?).map(|i| list.GetAt(i).map(|s| s.to_string()).unwrap_or_default()).collect();
        writeln!(out, "{reading}\t{:?}\t{}", t.elapsed(), items.join(" ")).unwrap();
    }
    Ok(())
}
