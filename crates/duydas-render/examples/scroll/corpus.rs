//! Synthetic message text: Turkish, Japanese and English, short to multi-paragraph. The same
//! corpus as the ADR 0018 spike, so results compare.

pub(crate) const AUTHORS: &[&str] = &[
    "Ayşe Yılmaz",
    "Çağrı Öztürk",
    "İlkay Doğan",
    "佐藤 美咲",
    "山田 太郎",
    "Sam Rivera",
    "Ömer Şahin",
    "高橋 ひなた",
];

pub(crate) const PHRASES: &[&str] = &[
    "Toplantı yarın saat 14.00'te, gündem dokümanını kanala bıraktım.",
    "Iğdır'dan İstanbul'a giden ilk uçak kaçta kalkıyor, bilen var mı?",
    "Şu PR'ı inceleyebilir misin? Ölçümler değişti, grafikleri güncelledim.",
    "ılık İzmir akşamı, ışıklar yanınca çay ocağı doldu.",
    "明日の会議は14時からです。資料はチャンネルに置きました。",
    "このバグは再現できました。ログを添付しますので確認をお願いします。",
    "日本語入力で変換中にエンターを押すと、確定だけされて送信されないことを確認しました。",
    "東京は今日も雨。傘を忘れずに。",
    "The release candidate passed on all three platforms.",
    "Can someone double-check the migration plan before Friday?",
    "lol 😄 that's exactly what happened last time",
    "Pushing the fix now; CI should be green in ten minutes.",
    "Performans bütçesi aşıldı mı diye her sürümde kontrol ediyoruz, ama bu sefer kaydırma testi ilk kez 10 bin mesajla koşuyor ve sonuçları ADR'a yazacağız.",
    "長いメッセージの折り返しが正しく動作するかを確認するために、この文はわざと長めに書いています。句読点の位置や禁則処理も見てください。",
];
