//! Theme taxonomy + lexicons (AI_SKILLS §6.5 four-point sets).

pub struct ThemeSpec {
    pub name: &'static str,
    pub subjective: &'static [&'static str],
    pub spend_categories: &'static [&'static str],
    pub calendar_keywords: &'static [&'static str],
    pub line_keywords: &'static [&'static str],
}

pub const THEMES: &[ThemeSpec] = &[
    ThemeSpec {
        name: "キャリア・仕事の将来",
        subjective: &[
            "キャリア", "転職", "将来", "焦", "昇進", "市場価値", "このままで",
            "方向転換", "スキル不足", "評価され", "就活", "ES", "エントリーシート",
            "面接", "選考", "内定", "ガクチカ", "業界", "説明会", "インターン",
        ],
        spend_categories: &[
            "書籍", "教材", "セミナー", "講座", "資格", "スクール", "勉強", "研修",
            "スーツ", "証明写真", "就活", "対策本",
        ],
        calendar_keywords: &[
            "勉強会", "面談", "面接", "セミナー", "講座", "資格", "LT", "カンファレンス",
            "説明会", "選考", "インターン", "ES", "OB訪問", "座談会",
        ],
        line_keywords: &[
            "転職", "エージェント", "勉強会", "面接", "応募", "内定", "選考", "説明会",
            "ES", "インターン",
        ],
    },
    ThemeSpec {
        name: "学習・自己投資",
        subjective: &[
            "学び", "学習", "勉強", "理解", "習得", "読みたい", "身につけ", "極め",
        ],
        spend_categories: &["書籍", "教材", "講座", "サブスク(学習)", "勉強"],
        calendar_keywords: &["勉強", "読書", "学習", "講座", "写経"],
        line_keywords: &["読んだ", "勉強して", "写経", "学んだ", "記事"],
    },
    ThemeSpec {
        name: "健康・身体",
        subjective: &[
            "健康", "運動", "睡眠", "疲", "体調", "痩せ", "体重", "休息", "休養", "寝不足",
        ],
        spend_categories: &["ジム", "医療", "薬", "サプリ", "スポーツ", "病院"],
        calendar_keywords: &["ジム", "ランニング", "筋トレ", "病院", "健診", "ヨガ", "散歩"],
        line_keywords: &["走った", "ジム", "筋トレ", "朝ラン", "病院"],
    },
    ThemeSpec {
        name: "人間関係・家族",
        subjective: &[
            "家族", "妻", "夫", "友人", "孤独", "会いたい", "人間関係", "感謝", "一緒に",
        ],
        spend_categories: &["交際費", "プレゼント", "外食", "飲み会", "旅行"],
        calendar_keywords: &["飲み会", "食事", "デート", "帰省", "旅行", "会う"],
        line_keywords: &["行くよ", "参加する", "会おう", "楽しみ", "ありがとう"],
    },
    ThemeSpec {
        name: "娯楽・消費",
        subjective: &["ゲーム", "動画", "趣味", "欲しい", "買いたい", "浪費", "無駄遣い"],
        spend_categories: &["娯楽", "ゲーム", "趣味", "サブスク", "課金", "ガチャ", "買い物"],
        calendar_keywords: &["ゲーム", "映画", "ライブ", "観戦"],
        line_keywords: &["買った", "課金", "ポチった", "届いた"],
    },
    ThemeSpec {
        name: "金銭・経済的安定",
        subjective: &[
            "貯金", "お金", "節約", "投資", "資産", "収入", "家計", "金銭", "経済的",
        ],
        spend_categories: &["投資", "貯蓄", "積立", "保険", "NISA"],
        calendar_keywords: &["銀行", "証券", "FP", "確定申告"],
        line_keywords: &["積立", "投資", "節約", "NISA"],
    },
    ThemeSpec {
        name: "課外活動・組織運営",
        subjective: &[
            "部活", "サークル", "マネジメント", "後輩", "主将", "幹部", "部員", "組織",
            "運営", "チームを",
        ],
        spend_categories: &["部費", "合宿", "遠征", "サークル", "大会", "ユニフォーム"],
        calendar_keywords: &[
            "部活", "練習", "合宿", "大会", "サークル", "ミーティング", "新歓",
        ],
        line_keywords: &["練習", "合宿", "部員", "後輩", "大会", "シフト", "集合"],
    },
    ThemeSpec {
        name: "技術開発・ものづくり",
        subjective: &[
            "開発", "実装", "コード", "プログラミング", "低レイヤ", "最適化", "OSS",
            "アルゴリズム", "作りたい",
        ],
        spend_categories: &["サーバー", "ドメイン", "開発", "PC", "キーボード", "部品", "基板"],
        calendar_keywords: &["開発", "ハッカソン", "競プロ", "コンテスト", "もくもく", "リリース"],
        line_keywords: &["実装した", "コミット", "リリース", "デバッグ", "競プロ", "動いた"],
    },
];

pub const INTENT_MARKERS: &[&str] = &[
    "したい", "しなきゃ", "べき", "焦", "不安", "やらないと", "つもり", "目標", "なりたい",
    "目指",
];

pub const GAP_THRESHOLD: f64 = 0.25;
pub const NEGLIGIBLE: f64 = 0.05;
pub const SIMULATED_PERSONA_WEIGHT: f64 = 0.1;
pub const GENUINE_DOC_MIN_WEIGHT: f64 = 0.5;

pub const TASK_LEXICON: &[(&str, &[&str])] = &[
    ("面接対策", &["面接対策", "面接練習", "模擬面接"]),
    ("ES執筆", &["ES", "エントリーシート"]),
    (
        "コーディングテスト対策",
        &["LeetCode", "リートコード", "競プロ", "コーディングテスト", "AtCoder", "過去問"],
    ),
    ("Webテスト対策", &["Webテスト", "SPI", "玉手箱"]),
    ("企業研究・OB訪問", &["企業研究", "業界研究", "OB訪問"]),
];

pub const DECLARE_MARKERS: &[&str] = &[
    "こそ", "やる", "やろう", "やらないと", "やらねば", "しなきゃ", "始める", "取り組む",
    "進める", "解く", "書く", "出す", "対策する", "受けよう",
];

pub const DONE_MARKERS: &[&str] = &[
    "やった", "解いた", "受けた", "終えた", "終わらせた", "提出した", "出した", "書き上げた",
    "行ってきた", "完了", "済ませた",
];

pub const K_HYPERBOLIC: f64 = 0.3;
pub const AVOIDANCE_FLAG_THRESHOLD: f64 = 0.5;

pub const ABSTRACT_LEXICON: &[&str] = &[
    "本質", "アーキテクチャ", "哲学", "長期的視野", "抽象", "概念", "構造的", "メタ", "俯瞰",
    "パラダイム", "方法論", "原理原則", "普遍", "体系", "あるべき姿", "自己実現",
];

pub const JOBHUNT_ACTION_KEYWORDS: &[&str] = &[
    "面接", "説明会", "選考", "ES", "エントリー", "OB訪問", "インターン", "テスト", "面談",
    "応募", "座談会",
];

pub const ABSTRACT_MIN_HITS: i32 = 3;
pub const ABSTRACT_SPIKE_RATIO: f64 = 1.5;

pub const PRIVATE_TIME_KEYWORDS: &[&str] = &[
    "デート", "パートナー", "彼女", "彼氏", "恋人", "記念日", "旅行", "食事", "飲み会", "会う",
];

pub const GUILT_MARKERS: &[&str] = &[
    "罪悪感", "生産性が落ち", "進まなかった", "進んでいない", "サボって", "無駄にした",
    "遊んでしまった", "勉強できなかった", "進まない", "やるべきだった",
];

pub const PRODUCTIVITY_MARKERS: &[&str] = &[
    "捗った", "はかどった", "集中できた", "一気に進んだ", "効率よく", "実装が進んだ", "解けた",
    "乗ってきた", "スラスラ", "冴えて",
];

pub const STABILIZER_WINDOW_DAYS: i64 = 2;
