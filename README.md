<a id="top"></a>
# Duydaş

A lightweight, secure, open-source place for communities to talk: text channels and voice, as a native Rust desktop app.

[![CI](https://github.com/IACBI/duydas/actions/workflows/ci.yml/badge.svg)](https://github.com/IACBI/duydas/actions/workflows/ci.yml)
[![CodeQL](https://github.com/IACBI/duydas/actions/workflows/codeql.yml/badge.svg)](https://github.com/IACBI/duydas/actions/workflows/codeql.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**Read this in:** [English](#english) · [Türkçe](#turkce)

---
<a id="english"></a>
## English

### Overview

Duydaş is a desktop chat and voice app for communities, written entirely in Rust with the [Iced](https://iced.rs) GUI toolkit. It aims to start fast, stay small in memory, run the same on Windows, macOS and Linux, and treat your privacy as a requirement rather than a setting. The server is self-hostable and lives in the same repository.

The project is in **Phase 0 (foundations)**. Today you get the workspace, the localization system, a desktop shell that switches language live, and the CI and project docs. Messaging, communities and voice come in later phases; see [the architecture overview](docs/architecture.md).

### Features

Working now:

- English, Turkish and Japanese UI, picked from your system language and switchable instantly without a restart
- Locale-aware numbers, dates and Turkish casing (`istanbul` → `İSTANBUL`)
- A message composer that handles input methods (IME), so Enter finishes a Japanese composition instead of sending it
- A compile-time check that every UI string exists, plus CI that blocks untranslated or hardcoded text

Planned (see the roadmap in [docs/architecture.md](docs/architecture.md)): communities, channels and roles; durable message delivery; voice channels; end-to-end encrypted direct messages; signed releases with secure auto-update.

### Requirements

- Rust stable (the repository's `rust-toolchain.toml` selects it; minimum supported version 1.98)
- Windows 10+, macOS 13+, or a Linux desktop with Wayland or X11
- A GPU with Vulkan, Metal, DirectX 12 or OpenGL support (a software renderer is used as a fallback)

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

The language follows your system settings. Change it from the picker in the left panel; the whole UI updates immediately.

Developer tasks:

```sh
cargo xtask i18n-check   # validate translations and look for hardcoded UI strings
cargo xtask perf         # measure startup time and memory of a release build
```

### Contributing

Contributions and translations are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md); translators should read [docs/i18n-guide.md](docs/i18n-guide.md). Please report security issues privately as described in [SECURITY.md](SECURITY.md). Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

### License

Licensed under the [Apache License 2.0](LICENSE). Created by 𝓐.𝓒.𝓑.

[⬆ Back to top](#top)

---
<a id="turkce"></a>
## Türkçe

### Genel Bakış

Duydaş, topluluklar için bir masaüstü sohbet ve sesli görüşme uygulaması. Tamamen Rust ile, [Iced](https://iced.rs) arayüz kütüphanesi kullanılarak yazılıyor. Hedeflerimiz: hızlı açılmak, az bellek kullanmak, Windows, macOS ve Linux'ta aynı şekilde çalışmak. Gizlilik bizim için bir ayar değil, temel bir gereksinim. Sunucuyu kendiniz barındırabilirsiniz ve kodu aynı depoda duruyor.

Proje şu an **Faz 0'da (temeller)**. Bugün elinizde çalışma alanı, yerelleştirme sistemi, dili anında değiştirebilen bir masaüstü kabuğu, CI ve proje belgeleri var. Mesajlaşma, topluluklar ve sesli sohbet sonraki fazlarda geliyor; ayrıntılar için [mimari belgesine](docs/architecture.md) bakın.

### Özellikler

Şu an çalışanlar:

- İngilizce, Türkçe ve Japonca arayüz: sistem dilinize göre seçilir, yeniden başlatmadan anında değiştirilebilir
- Yerel ayara uygun sayı, tarih ve Türkçe büyük harf dönüşümü (`istanbul` → `İSTANBUL`)
- Giriş yöntemlerini (IME) doğru işleyen mesaj kutusu: Japonca yazarken Enter, mesajı göndermek yerine dönüştürmeyi tamamlar
- Her arayüz metninin var olduğunu derleme sırasında doğrulayan kontrol; çevrilmemiş ya da koda gömülmüş metinleri CI engeller

Planlananlar ([docs/architecture.md](docs/architecture.md) içindeki yol haritası): topluluklar, kanallar ve roller; kayıpsız mesaj iletimi; sesli kanallar; uçtan uca şifreli özel mesajlar; imzalı sürümler ve güvenli otomatik güncelleme.

### Gereksinimler

- Rust stable (depodaki `rust-toolchain.toml` bunu seçer; desteklenen en düşük sürüm 1.98)
- Windows 10+, macOS 13+ ya da Wayland veya X11 kullanan bir Linux masaüstü
- Vulkan, Metal, DirectX 12 veya OpenGL destekli bir ekran kartı (yoksa yazılım tabanlı çizim kullanılır)

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

Arayüz dili sistem ayarlarınızı izler. Sol paneldeki seçiciden değiştirdiğinizde tüm arayüz hemen güncellenir.

Geliştirici komutları:

```sh
cargo xtask i18n-check   # validate translations and look for hardcoded UI strings
cargo xtask perf         # measure startup time and memory of a release build
```

### Katkı

Kod katkılarına ve çevirilere açığız. [CONTRIBUTING.md](CONTRIBUTING.md) ile başlayın; çevirmenler [docs/i18n-guide.md](docs/i18n-guide.md) belgesini okumalı. Güvenlik açıklarını [SECURITY.md](SECURITY.md) içinde anlatıldığı gibi gizli olarak bildirin. Katılan herkes [Davranış Kuralları](CODE_OF_CONDUCT.md)'na uyar.

### Lisans

[Apache Lisansı 2.0](LICENSE) ile lisanslanmıştır. 𝓐.𝓒.𝓑 tarafından geliştirilmektedir.

[⬆ Başa Dön](#top)
