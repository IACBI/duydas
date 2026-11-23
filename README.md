<a id="top"></a>
# Duydaş

Open-source community chat and voice for the desktop, written in Rust. Early development.

[![CI](https://github.com/IACBI/duydas/actions/workflows/ci.yml/badge.svg)](https://github.com/IACBI/duydas/actions/workflows/ci.yml)
[![CodeQL](https://github.com/IACBI/duydas/actions/workflows/codeql.yml/badge.svg)](https://github.com/IACBI/duydas/actions/workflows/codeql.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**Read this in:** [English](#english) · [Türkçe](#turkce)

---
<a id="english"></a>
## English

### Overview

Duydaş is a desktop app for community chat and voice. The goals are a fast start, a small memory footprint, the same behaviour on Windows, macOS and Linux, and privacy as a requirement rather than a setting. A self-hostable server is planned in this repository.

The project is in **Phase 0 (foundations)**. You can't chat with anyone yet. Today's desktop shell shows the localization system and a message composer whose messages stay on your screen. Most of the work goes into the layers underneath. Under [ADR 0021](docs/adr/0021-in-house-implementation.md), Duydaş builds its own platform, rendering, text and UI layers. In the long run, only the Opus codec and audited cryptography come from outside. Until the in-house toolkit is ready, the shell runs on [Iced](https://iced.rs). Messaging, communities and voice come in later phases; see [the architecture overview](docs/architecture.md).

### Features

In the desktop shell:

- English, Turkish and Japanese UI. The app starts in the first supported language from your system settings, falling back to English. The picker on the left switches the language at once, for the current session.
- Locale-aware numbers, dates and Turkish casing (`istanbul` → `İSTANBUL`), from our own Fluent-syntax parser and hand-written CLDR rules.
- A composer that does not send on Enter while an IME composition is in progress, so Enter can confirm Japanese input. Testing with real input methods is still pending ([ADR 0015](docs/adr/0015-spike-text-and-ime.md)).
- UI code refers to messages through keys generated from the English source, so a typo doesn't compile. CI checks that every locale has every message and flags string literals passed straight to UI functions.

In-house components, each with its own tests and an example to try:

- `duydas-sys`: a Win32 window layer with keyboard and IME input, clipboard, per-monitor DPI, and presentation of only the changed areas. On Windows 11 its example shows the first frame 14 ms after `main` (median of 21 runs).
- `duydas-render`: a CPU renderer that repaints only what changed and scrolls by moving pixels. In a headless replay of scrolling through 10,000 messages, p99 frame time is 1.5 ms at 1× and 2.5 ms at 2× (median of five runs; the budget is 16.7 ms). Text is still drawn with placeholder glyphs.
- `duydas-text`: Unicode 18.0 grapheme clusters, word boundaries and line breaking with Japanese rules. It passes all 853 grapheme, 1,944 word and 19,346 line break cases of Unicode's conformance tests.

Next on the platform side ([ADR 0021](docs/adr/0021-in-house-implementation.md)): Wayland/X11 and Cocoa backends, fonts, text shaping and layout, and the in-house UI toolkit. Planned product features ([roadmap](docs/architecture.md#roadmap)): communities, channels and roles; message delivery that survives reconnects; voice channels; end-to-end encrypted direct messages; signed releases with verified updates.

### Requirements

- Stable Rust, selected by `rust-toolchain.toml`. The minimum supported version is 1.98.
- The desktop app targets Windows 10+, macOS 13+, and Linux desktops with Wayland or X11. Iced draws with the GPU where it can and falls back to software rendering.
- The in-house examples below need Windows 10 version 1703 or later.

### Installation

There are no releases yet. Build from source:

```sh
git clone https://github.com/IACBI/duydas.git
cd duydas
cargo build --release
```

### Usage

```sh
cargo run --release
```

To try the in-house components on Windows:

```sh
cargo run --release -p duydas-sys --example window                  # window, keyboard input and IME
cargo run --release -p duydas-render --example scroll -- --window   # scrolling benchmark in a window
```

Developer tasks:

```sh
cargo xtask i18n-check     # check translations and look for hardcoded UI text
cargo xtask perf           # measure startup time and memory of a release build
cargo xtask render-bench   # check the renderer against the scrolling budget
cargo xtask vlist          # run the virtual list spike (ADR 0018)
cargo xtask unicode-gen    # regenerate the Unicode tables and conformance data
```

`unicode-gen` downloads the Unicode Character Database with `curl` the first time it runs.

### Contributing

Contributions and translations are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md); translators should read [docs/i18n-guide.md](docs/i18n-guide.md). Report security issues privately as described in [SECURITY.md](SECURITY.md). Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

### License

Licensed under the [Apache License 2.0](LICENSE). Unicode data in `crates/duydas-text` is used under the Unicode License v3; see [NOTICE](NOTICE). Created by 𝓐.𝓒.𝓑.

[⬆ Back to top](#top)

---
<a id="turkce"></a>
## Türkçe

### Genel Bakış

Duydaş, topluluklar için yazılı ve sesli sohbet sunan bir masaüstü uygulaması. Hedefimiz hızlı açılan, az bellek kullanan, Windows, macOS ve Linux'ta aynı şekilde çalışan bir uygulama. Gizlilik bizim için bir ayar değil, tasarımın temel gereksinimlerinden biri. Kendi altyapınızda barındırabileceğiniz sunucu uygulamasını da bu depoda geliştirmeyi planlıyoruz.

Proje şu an **0. aşamada (temeller)**. Henüz kimseyle sohbet edemiyorsunuz. Bugünkü masaüstü uygulamasında yerelleştirme sistemini ve mesaj yazma alanını deneyebilirsiniz; yazdığınız mesajlar yalnızca kendi ekranınızda görünür. Çalışmaların büyük bölümü uygulamanın temel katmanlarına gidiyor: [ADR 0021](docs/adr/0021-in-house-implementation.md) doğrultusunda platform, çizim, metin ve arayüz katmanlarını kendimiz yazıyoruz. Uzun vadede dışarıdan yalnızca Opus ses kodeği ve denetlenmiş kriptografi kütüphaneleri alınacak. Kendi arayüz katmanımız hazır olana kadar uygulama [Iced](https://iced.rs) üzerinde çalışıyor. Mesajlaşma, topluluklar ve sesli sohbet sonraki aşamalarda geliyor; ayrıntılar [mimari belgesinde](docs/architecture.md).

### Özellikler

Masaüstü uygulamasında:

- İngilizce, Türkçe ve Japonca arayüz. Uygulama, sistem ayarlarınızdaki dillerden desteklenen ilkiyle açılır; eşleşme yoksa İngilizce açılır. Soldaki seçiciden dili değiştirdiğinizde arayüz o oturum için hemen güncellenir.
- Kendi Fluent sözdizimi ayrıştırıcımız ve elle yazılmış CLDR kurallarıyla dile uygun sayılar, tarihler ve Türkçe büyük harf dönüşümü (`istanbul` → `İSTANBUL`).
- IME ile yazı girilirken Enter'a basıldığında mesajı göndermeyen, böylece Japonca girişin Enter ile onaylanmasına izin veren mesaj alanı. Gerçek giriş yöntemleriyle testi henüz yapılmadı ([ADR 0015](docs/adr/0015-spike-text-and-ime.md)).
- Arayüz kodu metinlere İngilizce kaynaktan üretilen anahtarlarla erişir; yanlış yazılmış bir anahtar derlenmez. CI her dilde her metnin bulunduğunu denetler ve arayüz işlevlerine doğrudan yazılan metinleri işaretler.

Kendi yazdığımız bileşenler (her birinin testleri ve deneyebileceğiniz bir örneği var):

- `duydas-sys`: Win32 pencere katmanı. Klavye ve IME girişini, panoyu ve monitöre göre DPI ölçeklendirmesini destekler; ekranda yalnızca değişen bölgeleri günceller. Windows 11'de örnek uygulamanın ilk karesi, `main` başladıktan 14 ms sonra ekrana geliyor (21 çalıştırmanın ortancası).
- `duydas-render`: yalnızca değişen yerleri yeniden çizen, kaydırırken pikselleri taşıyan CPU çizim motoru. 10.000 mesajlık bir listede kaydırmayı pencere açmadan tekrarlayan ölçümde p99 kare süresi 1× ölçekte 1,5 ms, 2× ölçekte 2,5 ms (beş denemenin ortancası; hedef 16,7 ms). Metin şimdilik temsili gliflerle çiziliyor.
- `duydas-text`: Unicode 18.0'a göre grafem kümelerini, sözcük sınırlarını ve Japonca satır bölme kurallarına göre satırın nerede bölünebileceğini bulur. Unicode'un uyumluluk testlerindeki 853 grafem, 1.944 sözcük ve 19.346 satır sonu örneğinin tamamını geçiyor.

Platform tarafında sırada ([ADR 0021](docs/adr/0021-in-house-implementation.md)) Wayland/X11 ve Cocoa desteği, yazı tipleri, metin şekillendirme ve yerleşim, ardından kendi arayüz katmanımız var. Planlanan özellikler ([yol haritası](docs/architecture.md#roadmap)): topluluklar, kanallar ve roller; bağlantı kopup geri geldiğinde mesaj kaybetmeyen iletim; sesli kanallar; uçtan uca şifreli özel mesajlar; imzalı sürümler ve doğrulanan güncellemeler.

### Gereksinimler

- Rust'ın kararlı (stable) kanalı; `rust-toolchain.toml` bunu seçer. Desteklenen en düşük sürüm 1.98.
- Masaüstü uygulaması Windows 10 ve sonrasını, macOS 13 ve sonrasını, Wayland ya da X11 kullanan Linux masaüstlerini hedefliyor. Iced, mümkünse ekran kartıyla çizer; olmazsa yazılımsal çizime geçer.
- Aşağıdaki örnekler için Windows 10 sürüm 1703 veya sonrası gerekir.

### Kurulum

Henüz yayımlanmış bir sürüm yok. Kaynaktan derleyin:

```sh
git clone https://github.com/IACBI/duydas.git
cd duydas
cargo build --release
```

### Kullanım

```sh
cargo run --release
```

Kendi yazdığımız bileşenleri Windows'ta denemek için:

```sh
cargo run --release -p duydas-sys --example window                  # pencere, klavye girişi ve IME
cargo run --release -p duydas-render --example scroll -- --window   # kaydırma ölçümü bir pencerede
```

Geliştirici komutları:

```sh
cargo xtask i18n-check     # çevirileri denetle, koda gömülü arayüz metinlerini ara
cargo xtask perf           # sürüm derlemesinin açılış süresini ve bellek kullanımını ölç
cargo xtask render-bench   # çizim motorunu kaydırma hedefine karşı sına
cargo xtask vlist          # sanal liste denemesini çalıştır (ADR 0018)
cargo xtask unicode-gen    # Unicode tablolarını ve uyumluluk verisini yeniden üret
```

`unicode-gen`, ilk çalıştığında Unicode Karakter Veritabanı'nı `curl` ile indirir.

### Katkı

Katkılara ve çevirilere açığız. Başlamak için [katkı rehberine](CONTRIBUTING.md), çeviri için [yerelleştirme rehberine](docs/i18n-guide.md) bakın. Güvenlik açıklarını [güvenlik politikasında](SECURITY.md) anlatılan özel kanaldan bildirin. Katılan herkes [davranış kurallarına](CODE_OF_CONDUCT.md) uyar.

### Lisans

[Apache Lisansı 2.0](LICENSE) ile lisanslanmıştır. `crates/duydas-text` içindeki Unicode verisi Unicode License v3 ile kullanılıyor; ayrıntılar [NOTICE](NOTICE) dosyasında. Projenin kurucusu 𝓐.𝓒.𝓑.

[⬆ Başa Dön](#top)
