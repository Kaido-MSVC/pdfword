//! PDF 字型名稱 → Word 字型（西文、東亞）。

/// 對應結果：(西文字型, 東亞字型)
pub fn map_font(pdf_name: &str) -> (String, String) {
    let base = base_name(pdf_name);
    let key: String = base.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let has = |k: &str| key.contains(k) || base.contains(k);

    // 中日韓字型
    let cjk: Option<&str> = if has("pmingliu") || has("mingliu") || has("新細明體") || has("細明體") {
        Some("新細明體")
    } else if has("dfkai") || has("biaukai") || has("標楷體") || has("kaiu") {
        Some("標楷體")
    } else if has("jhenghei") || has("正黑") {
        Some("微軟正黑體")
    } else if has("yahei") || has("雅黑") {
        Some("Microsoft YaHei")
    } else if has("simsun") || has("宋体") {
        Some("SimSun")
    } else if has("simhei") || has("黑体") {
        Some("SimHei")
    } else if has("kaiti") || has("楷体") {
        Some("KaiTi")
    } else if has("msmincho") {
        Some("MS Mincho")
    } else if has("msgothic") {
        Some("MS Gothic")
    } else if has("malgun") {
        Some("Malgun Gothic")
    } else if has("notosanscjk") || has("notosanstc") || has("sourcehansans") {
        Some("Noto Sans CJK TC")
    } else if has("notoserifcjk") || has("notoseriftc") || has("sourcehanserif") {
        Some("Noto Serif CJK TC")
    } else {
        None
    };
    if let Some(c) = cjk {
        return (c.to_string(), c.to_string());
    }

    let latin: Option<&str> = if key.starts_with("arial") || key.starts_with("helvetica") {
        Some("Arial")
    } else if key.starts_with("timesnewroman") || key.starts_with("times") {
        Some("Times New Roman")
    } else if key.starts_with("couriernew") || key.starts_with("courier") {
        Some("Courier New")
    } else if key.starts_with("calibri") {
        Some("Calibri")
    } else if key.starts_with("cambria") {
        Some("Cambria")
    } else if key.starts_with("segoeui") {
        Some("Segoe UI")
    } else if key.starts_with("verdana") {
        Some("Verdana")
    } else if key.starts_with("tahoma") {
        Some("Tahoma")
    } else if key.starts_with("georgia") {
        Some("Georgia")
    } else if key.starts_with("consolas") {
        Some("Consolas")
    } else if key.starts_with("symbol") {
        Some("Symbol")
    } else {
        None
    };
    let latin = match latin {
        Some(l) => l.to_string(),
        // 無法辨識（例如 Type3 或無名稱）就用預設
        None if base.is_empty() || key.starts_with("unnamed") || key.starts_with("t3font") => DEFAULT_LATIN.to_string(),
        None => base.clone(),
    };
    (latin, DEFAULT_EAST_ASIA.to_string())
}

pub const DEFAULT_LATIN: &str = "Calibri";
pub const DEFAULT_EAST_ASIA: &str = "新細明體";

/// 去掉樣式字尾（-Bold、,Italic、PSMT 等）
fn base_name(name: &str) -> String {
    let mut n = name.trim();
    if let Some((a, _)) = n.split_once([',', '-']) {
        n = a;
    }
    let mut s = n.to_string();
    for suf in ["PSMT", "MT", "PS", "Regular", "Bold", "Italic"] {
        if s.len() > suf.len() && s.ends_with(suf) {
            s.truncate(s.len() - suf.len());
        }
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_mappings() {
        assert_eq!(map_font("TimesNewRomanPSMT").0, "Times New Roman");
        assert_eq!(map_font("Arial-BoldMT").0, "Arial");
        assert_eq!(map_font("PMingLiU").1, "新細明體");
        assert_eq!(map_font("DFKaiShu-SB-Estd-BF").1, "標楷體");
        assert_eq!(map_font("MicrosoftJhengHei").1, "微軟正黑體");
        assert_eq!(map_font("Unnamed-T3"), (DEFAULT_LATIN.to_string(), DEFAULT_EAST_ASIA.to_string()));
        assert_eq!(map_font("Garamond"), ("Garamond".to_string(), DEFAULT_EAST_ASIA.to_string()));
    }
}
