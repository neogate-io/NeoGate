use anyhow::{Context, Result};
use lettre::{
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};

use crate::config::EmailConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmailLocale {
    ZhCn,
    EnUs,
}

impl EmailLocale {
    pub fn from_public_locale(locale: Option<&str>) -> Self {
        let Some(locale) = locale.map(str::trim).filter(|value| !value.is_empty()) else {
            return Self::ZhCn;
        };
        let locale = locale.replace('_', "-").to_ascii_lowercase();
        if locale == "en" || locale.starts_with("en-") {
            Self::EnUs
        } else {
            Self::ZhCn
        }
    }

    fn product_name(self) -> &'static str {
        "NeoGate"
    }
}

#[derive(Clone)]
pub struct EmailService {
    config: EmailConfig,
    mailer: AsyncSmtpTransport<Tokio1Executor>,
}

impl EmailService {
    pub fn new(config: EmailConfig) -> Result<Self> {
        let mut builder = if config.smtp_tls && config.smtp_port == 465 {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
                .context("failed to configure SMTPS relay")?
        } else if config.smtp_tls {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)
                .context("failed to configure STARTTLS SMTP relay")?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
        }
        .port(config.smtp_port);

        if let (Some(username), Some(password)) =
            (config.smtp_username.clone(), config.smtp_password.clone())
        {
            builder = builder.credentials(Credentials::new(username, password));
        }

        Ok(Self {
            config,
            mailer: builder.build(),
        })
    }

    pub fn test() -> Self {
        let config = EmailConfig {
            smtp_host: "localhost".to_string(),
            smtp_port: 2525,
            smtp_username: None,
            smtp_password: None,
            smtp_tls: false,
            from_email: "noreply@example.com".to_string(),
            from_name: None,
            subject_prefix: None,
        };
        Self {
            mailer: AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("localhost")
                .port(2525)
                .build(),
            config,
        }
    }

    pub async fn send_api_key(
        &self,
        to_email: &str,
        api_key: &str,
        locale: EmailLocale,
    ) -> Result<()> {
        let message = self.api_key_message(to_email, api_key, locale)?;

        self.mailer
            .send(message)
            .await
            .context("failed to send API key email")?;

        Ok(())
    }

    pub async fn send_password_reset(
        &self,
        to_email: &str,
        reset_url: &str,
        locale: EmailLocale,
    ) -> Result<()> {
        let message = self.password_reset_message(to_email, reset_url, locale)?;

        self.mailer
            .send(message)
            .await
            .context("failed to send password reset email")?;

        Ok(())
    }

    pub async fn send_login_verification_code(
        &self,
        to_email: &str,
        code: &str,
        locale: EmailLocale,
    ) -> Result<()> {
        let message = self.login_verification_code_message(to_email, code, locale)?;

        self.mailer
            .send(message)
            .await
            .context("failed to send login verification code email")?;

        Ok(())
    }

    fn sender_mailbox(&self, locale: EmailLocale) -> Result<Mailbox> {
        Ok(Mailbox::new(
            Some(
                self.config
                    .from_name
                    .clone()
                    .unwrap_or_else(|| locale.product_name().to_string()),
            ),
            self.config
                .from_email
                .parse()
                .context("MAIL_FROM_EMAIL is invalid")?,
        ))
    }

    fn api_key_message(
        &self,
        to_email: &str,
        api_key: &str,
        locale: EmailLocale,
    ) -> Result<Message> {
        let content = api_key_email_content(locale);
        let subject_prefix = self
            .config
            .subject_prefix
            .as_deref()
            .unwrap_or_else(|| locale.product_name());
        let subject = api_key_email_subject(subject_prefix, content);
        Message::builder()
            .from(self.sender_mailbox(locale)?)
            .to(to_email
                .parse::<Mailbox>()
                .context("recipient email address is invalid")?)
            .subject(subject)
            .multipart(MultiPart::alternative_plain_html(
                api_key_email_text_body(api_key, content),
                api_key_email_html_body(api_key, content),
            ))
            .context("failed to build API key email")
    }

    fn password_reset_message(
        &self,
        to_email: &str,
        reset_url: &str,
        locale: EmailLocale,
    ) -> Result<Message> {
        let content = password_reset_email_content(locale);
        let subject_prefix = self
            .config
            .subject_prefix
            .as_deref()
            .unwrap_or_else(|| locale.product_name());
        Message::builder()
            .from(self.sender_mailbox(locale)?)
            .to(to_email
                .parse::<Mailbox>()
                .context("recipient email address is invalid")?)
            .subject(format!("{subject_prefix} {}", content.subject))
            .multipart(MultiPart::alternative_plain_html(
                password_reset_email_text_body(reset_url, content),
                password_reset_email_html_body(reset_url, content),
            ))
            .context("failed to build password reset email")
    }

    fn login_verification_code_message(
        &self,
        to_email: &str,
        code: &str,
        locale: EmailLocale,
    ) -> Result<Message> {
        let content = login_verification_code_email_content(locale);
        let subject_prefix = self
            .config
            .subject_prefix
            .as_deref()
            .unwrap_or_else(|| locale.product_name());
        Message::builder()
            .from(self.sender_mailbox(locale)?)
            .to(to_email
                .parse::<Mailbox>()
                .context("recipient email address is invalid")?)
            .subject(format!("{subject_prefix} {}", content.subject))
            .multipart(MultiPart::alternative_plain_html(
                login_verification_code_email_text_body(code, content),
                login_verification_code_email_html_body(code, content),
            ))
            .context("failed to build login verification code email")
    }
}

#[derive(Clone, Copy)]
struct ApiKeyEmailContent {
    subject: &'static str,
    title: &'static str,
    intro: &'static str,
    api_key_label: &'static str,
    footer: &'static str,
}

fn api_key_email_content(locale: EmailLocale) -> ApiKeyEmailContent {
    match locale {
        EmailLocale::ZhCn => ApiKeyEmailContent {
            subject: "API 密钥",
            title: "你的 NeoGate API 密钥已生成",
            intro: "调用 NeoGate 时请使用下面的密钥。请妥善保管，完整密钥只会通过这封邮件发送。",
            api_key_label: "API 密钥",
            footer: "如果这不是你本人操作，请在 NeoGate 管理后台轮换该密钥。",
        },
        EmailLocale::EnUs => ApiKeyEmailContent {
            subject: "API Key",
            title: "Your NeoGate API key is ready",
            intro: "Use this key when calling NeoGate. Keep it private because the full key is only sent in this email.",
            api_key_label: "API key",
            footer: "If you did not request this key, rotate it from the NeoGate admin console.",
        },
    }
}

fn api_key_email_subject(prefix: &str, content: ApiKeyEmailContent) -> String {
    format!("{prefix} {}", content.subject)
}

#[derive(Clone, Copy)]
struct PasswordResetEmailContent {
    subject: &'static str,
    title: &'static str,
    intro: &'static str,
    action: &'static str,
    footer: &'static str,
}

#[derive(Clone, Copy)]
struct LoginVerificationCodeEmailContent {
    subject: &'static str,
    title: &'static str,
    intro: &'static str,
    code_label: &'static str,
    footer: &'static str,
}

fn password_reset_email_content(locale: EmailLocale) -> PasswordResetEmailContent {
    match locale {
        EmailLocale::ZhCn => PasswordResetEmailContent {
            subject: "重置密码",
            title: "重置你的 NeoGate 密码",
            intro: "点击下面的按钮设置新的登录密码。链接会在 30 分钟后失效。",
            action: "重置密码",
            footer: "如果这不是你本人操作，可以忽略这封邮件。",
        },
        EmailLocale::EnUs => PasswordResetEmailContent {
            subject: "Reset password",
            title: "Reset your NeoGate password",
            intro: "Click the button below to set a new sign-in password. This link expires in 30 minutes.",
            action: "Reset password",
            footer: "If you did not request this, you can ignore this email.",
        },
    }
}

fn login_verification_code_email_content(locale: EmailLocale) -> LoginVerificationCodeEmailContent {
    match locale {
        EmailLocale::ZhCn => LoginVerificationCodeEmailContent {
            subject: "登录验证码",
            title: "验证你的 NeoGate 邮箱",
            intro: "请输入下面的验证码完成首次登录。验证码会在 10 分钟后失效。",
            code_label: "验证码",
            footer: "如果这不是你本人操作，可以忽略这封邮件。",
        },
        EmailLocale::EnUs => LoginVerificationCodeEmailContent {
            subject: "Sign-in verification code",
            title: "Verify your NeoGate email",
            intro: "Enter this code to finish your first sign-in. It expires in 10 minutes.",
            code_label: "Verification code",
            footer: "If you did not request this, you can ignore this email.",
        },
    }
}

fn api_key_email_text_body(api_key: &str, content: ApiKeyEmailContent) -> String {
    format!(
        "{}\n\n{}:\n{api_key}\n\n{}",
        content.title, content.api_key_label, content.intro
    )
}

fn api_key_email_html_body(api_key: &str, content: ApiKeyEmailContent) -> String {
    let escaped_api_key = escape_html(api_key);
    let escaped_title = escape_html(content.title);
    let escaped_intro = escape_html(content.intro);
    let escaped_footer = escape_html(content.footer);
    format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{escaped_title}</title>
  </head>
  <body style="margin:0;background:#f5f7fb;color:#172033;font-family:Arial,Helvetica,sans-serif;">
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="background:#f5f7fb;padding:32px 16px;">
      <tr>
        <td align="center">
          <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="max-width:560px;background:#ffffff;border:1px solid #e1e6ef;border-radius:8px;overflow:hidden;">
            <tr>
              <td style="padding:28px 32px 16px;">
                <h1 style="margin:0;color:#111827;font-size:24px;line-height:32px;font-weight:700;">{escaped_title}</h1>
                <p style="margin:12px 0 0;color:#4b5563;font-size:15px;line-height:24px;">{escaped_intro}</p>
              </td>
            </tr>
            <tr>
              <td style="padding:8px 32px 24px;">
                <div style="margin:0;padding:16px;background:#111827;border-radius:8px;color:#f9fafb;font-size:14px;line-height:22px;font-family:Consolas,Menlo,Monaco,monospace;word-break:break-all;">{escaped_api_key}</div>
              </td>
            </tr>
            <tr>
              <td style="padding:0 32px 28px;">
                <p style="margin:0;color:#6b7280;font-size:13px;line-height:20px;">{escaped_footer}</p>
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </body>
</html>"#
    )
}

fn login_verification_code_email_text_body(
    code: &str,
    content: LoginVerificationCodeEmailContent,
) -> String {
    format!(
        "{}\n\n{}:\n{code}\n\n{}\n\n{}",
        content.title, content.code_label, content.intro, content.footer
    )
}

fn login_verification_code_email_html_body(
    code: &str,
    content: LoginVerificationCodeEmailContent,
) -> String {
    let escaped_code = escape_html(code);
    let escaped_title = escape_html(content.title);
    let escaped_intro = escape_html(content.intro);
    let escaped_label = escape_html(content.code_label);
    let escaped_footer = escape_html(content.footer);
    format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{escaped_title}</title>
  </head>
  <body style="margin:0;background:#f5f7fb;color:#172033;font-family:Arial,Helvetica,sans-serif;">
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="background:#f5f7fb;padding:32px 16px;">
      <tr>
        <td align="center">
          <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="max-width:560px;background:#ffffff;border:1px solid #e1e6ef;border-radius:8px;overflow:hidden;">
            <tr>
              <td style="padding:28px 32px 16px;">
                <h1 style="margin:0;color:#111827;font-size:24px;line-height:32px;font-weight:700;">{escaped_title}</h1>
                <p style="margin:12px 0 0;color:#4b5563;font-size:15px;line-height:24px;">{escaped_intro}</p>
              </td>
            </tr>
            <tr>
              <td style="padding:8px 32px 24px;">
                <p style="margin:0 0 8px;color:#6b7280;font-size:13px;line-height:20px;">{escaped_label}</p>
                <div style="display:inline-block;letter-spacing:8px;margin:0;padding:14px 18px;background:#111827;border-radius:8px;color:#f9fafb;font-size:28px;line-height:34px;font-family:Consolas,Menlo,Monaco,monospace;">{escaped_code}</div>
              </td>
            </tr>
            <tr>
              <td style="padding:0 32px 28px;">
                <p style="margin:0;color:#6b7280;font-size:13px;line-height:20px;">{escaped_footer}</p>
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </body>
</html>"#
    )
}

fn password_reset_email_text_body(reset_url: &str, content: PasswordResetEmailContent) -> String {
    format!(
        "{}\n\n{}\n{reset_url}\n\n{}",
        content.title, content.intro, content.footer
    )
}

fn password_reset_email_html_body(reset_url: &str, content: PasswordResetEmailContent) -> String {
    let escaped_title = escape_html(content.title);
    let escaped_intro = escape_html(content.intro);
    let escaped_action = escape_html(content.action);
    let escaped_footer = escape_html(content.footer);
    let escaped_url = escape_html(reset_url);
    format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{escaped_title}</title>
  </head>
  <body style="margin:0;background:#f5f7fb;color:#172033;font-family:Arial,Helvetica,sans-serif;">
    <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="background:#f5f7fb;padding:32px 16px;">
      <tr>
        <td align="center">
          <table role="presentation" width="100%" cellspacing="0" cellpadding="0" style="max-width:560px;background:#ffffff;border:1px solid #e1e6ef;border-radius:8px;overflow:hidden;">
            <tr>
              <td style="padding:28px 32px 16px;">
                <h1 style="margin:0;color:#111827;font-size:24px;line-height:32px;font-weight:700;">{escaped_title}</h1>
                <p style="margin:12px 0 0;color:#4b5563;font-size:15px;line-height:24px;">{escaped_intro}</p>
              </td>
            </tr>
            <tr>
              <td style="padding:8px 32px 24px;">
                <a href="{escaped_url}" style="display:inline-block;background:#3f8cff;color:#ffffff;text-decoration:none;border-radius:8px;padding:12px 18px;font-size:15px;line-height:22px;">{escaped_action}</a>
              </td>
            </tr>
            <tr>
              <td style="padding:0 32px 28px;">
                <p style="margin:0 0 12px;color:#6b7280;font-size:13px;line-height:20px;word-break:break-all;">{escaped_url}</p>
                <p style="margin:0;color:#6b7280;font-size:13px;line-height:20px;">{escaped_footer}</p>
              </td>
            </tr>
          </table>
        </td>
      </tr>
    </table>
  </body>
</html>"#
    )
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_email_text_contains_full_key() {
        let body = api_key_email_text_body("sk-test123", api_key_email_content(EmailLocale::EnUs));

        assert!(body.contains("sk-test123"));
        assert!(body.contains("Your NeoGate API key is ready"));
    }

    #[test]
    fn api_key_email_html_contains_escaped_full_key() {
        let body =
            api_key_email_html_body("sk-test<&123", api_key_email_content(EmailLocale::EnUs));

        assert!(body.contains("sk-test&lt;&amp;123"));
        assert!(body.contains("Your NeoGate API key is ready"));
    }

    #[test]
    fn api_key_email_content_can_be_chinese() {
        let text = api_key_email_text_body("sk-test123", api_key_email_content(EmailLocale::ZhCn));
        let html = api_key_email_html_body("sk-test123", api_key_email_content(EmailLocale::ZhCn));

        assert!(text.contains("你的 NeoGate API 密钥已生成"));
        assert!(text.contains("sk-test123"));
        assert!(html.contains("你的 NeoGate API 密钥已生成"));
        assert!(html.contains("sk-test123"));
    }

    #[test]
    fn public_locale_defaults_to_chinese_and_accepts_english() {
        assert_eq!(EmailLocale::from_public_locale(None), EmailLocale::ZhCn);
        assert_eq!(EmailLocale::from_public_locale(Some("")), EmailLocale::ZhCn);
        assert_eq!(
            EmailLocale::from_public_locale(Some("zh-CN")),
            EmailLocale::ZhCn
        );
        assert_eq!(
            EmailLocale::from_public_locale(Some("en-US")),
            EmailLocale::EnUs
        );
        assert_eq!(
            EmailLocale::from_public_locale(Some("en")),
            EmailLocale::EnUs
        );
    }

    #[test]
    fn api_key_message_is_html_with_plain_text_part() {
        let service = EmailService::test();

        let message = service
            .api_key_message("user@example.com", "sk-test123", EmailLocale::ZhCn)
            .unwrap();
        let formatted = String::from_utf8(message.formatted()).unwrap();

        assert!(formatted.contains("Content-Type: multipart/alternative"));
        assert!(formatted.contains("Content-Type: text/plain"));
        assert!(formatted.contains("Content-Type: text/html"));
        assert!(formatted.contains("sk-test123"));
    }

    #[test]
    fn api_key_email_subject_uses_chinese_product_name() {
        assert_eq!(
            api_key_email_subject("NeoGate", api_key_email_content(EmailLocale::ZhCn)),
            "NeoGate API 密钥"
        );
    }

    #[test]
    fn api_key_email_subject_uses_english_product_name() {
        assert_eq!(
            api_key_email_subject("NeoGate", api_key_email_content(EmailLocale::EnUs)),
            "NeoGate API Key"
        );
    }

    #[test]
    fn password_reset_email_contains_reset_url() {
        let url = "https://example.com/reset-password?token=test";
        let text =
            password_reset_email_text_body(url, password_reset_email_content(EmailLocale::EnUs));
        let html =
            password_reset_email_html_body(url, password_reset_email_content(EmailLocale::EnUs));

        assert!(text.contains(url));
        assert!(text.contains("Reset your NeoGate password"));
        assert!(html.contains(url));
        assert!(html.contains("Reset your NeoGate password"));
    }

    #[test]
    fn login_verification_code_email_contains_code() {
        let text = login_verification_code_email_text_body(
            "123456",
            login_verification_code_email_content(EmailLocale::ZhCn),
        );
        let html = login_verification_code_email_html_body(
            "123456",
            login_verification_code_email_content(EmailLocale::ZhCn),
        );

        assert!(text.contains("123456"));
        assert!(text.contains("验证你的 NeoGate 邮箱"));
        assert!(html.contains("123456"));
        assert!(html.contains("验证你的 NeoGate 邮箱"));
    }

    #[test]
    fn product_name_follows_email_locale() {
        assert_eq!(EmailLocale::ZhCn.product_name(), "NeoGate");
        assert_eq!(EmailLocale::EnUs.product_name(), "NeoGate");
    }
}
