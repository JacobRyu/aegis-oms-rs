use crate::domain::error::{OmsError, Result};
use crate::domain::risk_engine::MarginLevel;
use rust_decimal::Decimal;

/// アラート情報
#[derive(Debug, Clone)]
pub struct Alert {
    pub level: MarginLevel,
    pub margin_ratio: Decimal,
    pub message: String,
}

impl Alert {
    pub fn new(level: MarginLevel, margin_ratio: Decimal) -> Self {
        let message = match level {
            MarginLevel::MarginCall => format!(
                "[追証警告] 証拠金率が {:.2}% に低下しました。追加入金またはポジション縮小をご検討ください。",
                margin_ratio
            ),
            MarginLevel::StopOut => format!(
                "[ロスカット] 証拠金率が {:.2}% に低下しました。全ポジションを強制決済します。",
                margin_ratio
            ),
            MarginLevel::Normal => format!("証拠金率: {:.2}%", margin_ratio),
        };
        Self { level, margin_ratio, message }
    }
}

/// アラート送信トレイト
pub trait AlertSender: Send + Sync {
    fn send(&self, alert: &Alert) -> Result<()>;
}

/// コンソール出力（デフォルト・テスト用）
pub struct ConsoleAlertSender;

impl AlertSender for ConsoleAlertSender {
    fn send(&self, alert: &Alert) -> Result<()> {
        eprintln!("[ALERT] {}", alert.message);
        Ok(())
    }
}

/// SMTP メール送信（lettre 経由）
///
/// 設定は環境変数から読み込む:
/// - `SMTP_HOST`  : SMTP サーバーホスト名
/// - `SMTP_PORT`  : SMTP ポート番号（デフォルト 587）
/// - `SMTP_USER`  : SMTP ユーザー名
/// - `SMTP_PASS`  : SMTP パスワード
/// - `ALERT_FROM` : 送信元メールアドレス
/// - `ALERT_TO`   : 送信先メールアドレス
pub struct SmtpAlertSender {
    smtp_host: String,
    smtp_port: u16,
    username: String,
    password: String,
    from_addr: String,
    to_addr: String,
}

impl SmtpAlertSender {
    /// 環境変数から設定を読み込む
    pub fn from_env() -> std::result::Result<Self, String> {
        let get = |key: &str| {
            std::env::var(key).map_err(|_| format!("環境変数 {key} が設定されていません"))
        };
        Ok(Self {
            smtp_host: get("SMTP_HOST")?,
            smtp_port: std::env::var("SMTP_PORT")
                .unwrap_or_else(|_| "587".into())
                .parse()
                .map_err(|_| "SMTP_PORT は数値で指定してください".to_string())?,
            username: get("SMTP_USER")?,
            password: get("SMTP_PASS")?,
            from_addr: get("ALERT_FROM")?,
            to_addr: get("ALERT_TO")?,
        })
    }
}

impl AlertSender for SmtpAlertSender {
    fn send(&self, alert: &Alert) -> Result<()> {
        use lettre::message::header::ContentType;
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{Message, SmtpTransport, Transport};

        let subject = match alert.level {
            MarginLevel::MarginCall => "【Aegis OMS】追証警告",
            MarginLevel::StopOut => "【Aegis OMS】ロスカット発動",
            MarginLevel::Normal => "【Aegis OMS】証拠金通知",
        };

        let email = Message::builder()
            .from(self.from_addr.parse().map_err(|e| OmsError::AlertFailed {
                reason: format!("送信元アドレス解析エラー: {e}"),
            })?)
            .to(self.to_addr.parse().map_err(|e| OmsError::AlertFailed {
                reason: format!("送信先アドレス解析エラー: {e}"),
            })?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(alert.message.clone())
            .map_err(|e| OmsError::AlertFailed {
                reason: format!("メール構築エラー: {e}")
            })?;

        let creds = Credentials::new(self.username.clone(), self.password.clone());

        let mailer = SmtpTransport::starttls_relay(&self.smtp_host)
            .map_err(|e| OmsError::AlertFailed { reason: format!("SMTP 接続エラー: {e}") })?
            .port(self.smtp_port)
            .credentials(creds)
            .build();

        mailer
            .send(&email)
            .map_err(|e| OmsError::AlertFailed { reason: format!("SMTP 送信エラー: {e}") })?;

        Ok(())
    }
}
