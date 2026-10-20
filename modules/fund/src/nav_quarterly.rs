//! Public quarterly holdings adapter; separate from NAV persistence.
use super::{EastMoneySource, NavError, QuarterlyHolding, QuarterlyReport, SOURCE_EASTMONEY};
#[path = "nav_quarterly_archives.rs"]
mod archives;

impl EastMoneySource {
    pub fn fetch_quarterly_positions(fund_code: &str) -> Result<QuarterlyReport, NavError> {
        if fund_code.len() != 6 || !fund_code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(NavError::BadPayload("基金代码须为 6 位数字".into()));
        }
        let value = crate::market::cached_public_response(
            format!("fund-quarter:{fund_code}"),
            std::time::Duration::from_secs(600),
            || {
                let mobile = Self::fetch_mobile_positions(fund_code);
                let report = archives::fetch(fund_code, mobile.as_ref().ok())
                    .or(mobile)
                    .map_err(|e| (502, e.message()))?;
                serde_json::to_string(&report)
                    .map_err(|_| (500, "Quarterly encoding failed".into()))
            },
        )
        .map_err(|(_, message)| NavError::Source(message))?;
        serde_json::from_str(&value).map_err(|_| NavError::BadPayload("季度缓存无效".into()))
    }

    fn fetch_mobile_positions(fund_code: &str) -> Result<QuarterlyReport, NavError> {
        let url = format!(
            "https://fundmobapi.eastmoney.com/FundMNewApi/FundMNInverstPosition?FCODE={code}&deviceid=natives-fund&plat=Iphone&product=EFund&version=6.2.8",
            code = fund_code
        );
        let agent = crate::network::client();
        let text = agent
            .get(&url)
            .set(
                "User-Agent",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) eastmoney/6.2.8",
            )
            .call()
            .map_err(|e| NavError::Source(format!("eastmoney: {e}")))?
            .into_string()
            .map_err(|e| NavError::Source(format!("eastmoney body: {e}")))?;
        Self::parse_quarterly_positions(&text, fund_code)
    }

    pub fn parse_quarterly_positions(
        text: &str,
        fund_code: &str,
    ) -> Result<QuarterlyReport, NavError> {
        let val: serde_json::Value =
            serde_json::from_str(text).map_err(|e| NavError::BadPayload(format!("json: {e}")))?;
        let err_code = val.get("ErrCode").and_then(|v| v.as_i64()).unwrap_or(-1);
        if err_code != 0 {
            return Err(NavError::Source(format!("eastmoney ErrCode {err_code}")));
        }
        let report_date = val
            .get("Expansion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let datas = val.get("Datas");
        if !datas.is_some_and(|d| d.is_object()) {
            return Err(NavError::BadPayload("季度披露缺少数据对象".into()));
        }
        let mut stocks = Vec::new();
        if let Some(stk_arr) = datas
            .and_then(|d| d.get("fundStocks"))
            .and_then(|v| v.as_array())
        {
            for item in stk_arr {
                stocks.push(QuarterlyHolding {
                    code: item
                        .get("GPDM")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: item
                        .get("GPJC")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    ratio_pct: item
                        .get("JZBL")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    change_type: item
                        .get("PCTNVCHGTYPE")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    change_pct: item
                        .get("PCTNVCHG")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    industry: item
                        .get("INDEXNAME")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    quote_symbol: None,
                    previous_ratio_pct: None,
                });
            }
        }
        let mut bonds = Vec::new();
        if let Some(bnd_arr) = datas
            .and_then(|d| d.get("fundboods"))
            .and_then(|v| v.as_array())
        {
            for item in bnd_arr {
                bonds.push(QuarterlyHolding {
                    code: item
                        .get("ZQDM")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: item
                        .get("ZQMC")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    ratio_pct: item
                        .get("ZJZBL")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    change_type: String::new(),
                    change_pct: String::new(),
                    industry: "债券".into(),
                    quote_symbol: None,
                    previous_ratio_pct: None,
                });
            }
        }
        Ok(QuarterlyReport {
            fund_code: fund_code.to_string(),
            report_date,
            source: SOURCE_EASTMONEY.into(),
            stocks,
            bonds,
            previous_report_date: None,
        })
    }
}
