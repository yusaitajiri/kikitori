//! Lists capture devices and the app picker: `cargo run --example devices`.

use kikitori_lib::audio::source::AudioAppLister;

fn main() -> anyhow::Result<()> {
    for d in kikitori_lib::audio::win::devices::list_mic_devices()? {
        println!("mic: {}{} [{}]", d.name, if d.is_default { " (default)" } else { "" }, d.id);
    }
    let t = std::time::Instant::now();
    let apps = kikitori_lib::audio::win::sessions::WinAudioAppLister.list()?;
    println!("listed {} apps in {:?}", apps.len(), t.elapsed());
    for a in apps {
        println!(
            "app: {:<28} pid {:>6} active={} session={} icon={} exe={}",
            a.name,
            a.root_pid,
            a.active,
            a.has_session,
            a.icon_data_url.is_some(),
            a.exe
        );
    }
    Ok(())
}
