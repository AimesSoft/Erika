//! Shared account profiles and login dialog. Provider data and commands
//! vary; avatars, typography, layout and dialog actions stay together.
use bern::prelude::*;
use erika_protocol::erika;

#[bern::session]
pub struct AccountLoginDraft {
    pub user: String,
    pub password: String,
    pub captcha: String,
}

#[component]
pub fn account_dialogs(app: erika::Client, settings_window: bool, #[session] draft: AccountLoginDraft) -> impl View {
    ui! {
        if app.features.account && app.bangumi.prompt && app.bangumi.settings_window == settings_window {
            account_login(app: app, bangumi: true, busy: app.bangumi.busy, message: app.bangumi.error)
        } else if app.features.account && app.account_status.prompt && app.account_status.settings_window == settings_window {
            account_login(app: app, bangumi: false, busy: app.account_status.busy, message: app.account_status.error)
        } else if app.features.account && app.bangumi.expired && !settings_window && !app.onboarding.visible {
            Modal(width: 420, appear: pop_in, disappear: fade) {
                Column(gap: 16, padding: 24) {
                    Text(content: tr! { "zh-Hans" => "Bangumi 需要重新登录", "en" => "Sign in to Bangumi again" }, style: title3)
                    Text(content: tr! { "zh-Hans" => "登录状态已失效，请重新填写密码和验证码。", "en" => "Your session has expired. Enter your password and a new verification code to sign in again." }, lines: 3)
                    Row(gap: 12, align: center) {
                        Spacer
                        Button(id: bangumi_later, label: tr! { "zh-Hans" => "稍后", "en" => "Later" }) => app.dismiss_bangumi_expired()
                        Button(id: bangumi_again, label: tr! { "zh-Hans" => "重新登录", "en" => "Sign In Again" }, prominence: prominent, clears: draft.password) => app.open_bangumi_login(false)
                    }
                }
            }
        }
    }
}

#[component]
pub fn account_login(
    app: erika::Client,
    bangumi: bool,
    busy: bool,
    #[content] message: String,
    #[session] draft: AccountLoginDraft,
) -> impl View {
    ui! {
        Modal(width: 400, appear: pop_in, disappear: fade) {
            Column(gap: 16, padding: 24) {
                if bangumi {
                    Text(content: tr! { "zh-Hans" => "登录 Bangumi", "en" => "Sign in to Bangumi" }, style: title3)
                } else {
                    Text(content: tr! { "zh-Hans" => "登录弹弹play", "en" => "Sign in to dandanplay" }, style: title3)
                }
                ScrollView {
                    Column(gap: 16) {
                        if bangumi {
                            TextInput(id: bangumi_email, label: tr! { "zh-Hans" => "邮箱", "en" => "Email" }, value: draft.user, prompt: tr! { "zh-Hans" => "Bangumi 注册邮箱", "en" => "Your Bangumi email" })
                        } else {
                            TextInput(id: account_user, label: tr! { "zh-Hans" => "用户名", "en" => "User name" }, value: draft.user, prompt: tr! { "zh-Hans" => "用户名或邮箱", "en" => "User name or email" })
                        }
                        TextInput(id: account_password, label: tr! { "zh-Hans" => "密码", "en" => "Password" }, value: draft.password, kind: secure, prompt: tr! { "zh-Hans" => "密码", "en" => "Password" })
                        if bangumi {
                            Column(gap: 10) {
                                Row(gap: 12, align: center) {
                                    Stack(width: 180, height: 64, align: center) {
                                        if let Some(captcha) = app.bangumi.captcha {
                                            Image(source: captcha, width: 180, height: 64, fit: contain, label: tr! { "zh-Hans" => "Bangumi 图片验证码", "en" => "Bangumi verification image" })
                                        } else {
                                            Text(content: tr! { "zh-Hans" => "图片验证码", "en" => "Verification image" }, emphasis: secondary)
                                        }
                                    }
                                    Spacer
                                    Button(id: bangumi_refresh, label: tr! { "zh-Hans" => "换一张", "en" => "Refresh" }, icon: "arrow.triangle.2.circlepath", enabled: !busy, clears: draft.captcha) => app.refresh_bangumi_captcha()
                                }
                                TextInput(id: bangumi_captcha, label: tr! { "zh-Hans" => "验证码", "en" => "Verification code" }, value: draft.captcha, prompt: tr! { "zh-Hans" => "填写图片中的字符", "en" => "Enter the characters in the image" })
                            }
                        }
                        if busy {
                            Text(content: tr! { "zh-Hans" => "正在连接…", "en" => "Connecting…" }, style: caption, emphasis: secondary)
                        } else if "" != message {
                            Text(content: message, style: caption, lines: 3)
                        }
                    }
                }
                Row(gap: 12, align: center) {
                    Spacer
                    Button(id: account_cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, role: cancel, prominence: plain, clears: draft.password, shortcut: app.keys.back) => app.cancel_account_login()
                    Button(id: account_sign_in, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, prominence: prominent, enabled: !busy) => app.submit_account_login(bangumi, draft.user, draft.password, draft.captcha)
                }
            }
        }
    }
}

/// Both providers use the same signed-in and signed-out account layout.
#[component]
pub fn account_profile(
    app: erika::Client,
    bangumi: bool,
    settings_window: bool,
    signed_in: bool,
    #[content] name: String,
    #[content] user: String,
    picture: Option<MediaSource>,
    busy: bool,
    #[content] message: String,
    #[session] draft: AccountLoginDraft,
) -> impl View {
    ui! {
        Column(gap: 12) {
            Row(gap: 12, align: center) {
                Stack(variant: avatar, width: 48, height: 48, align: center) {
                    Canvas(width: 48, height: 48) {
                        Shape(kind: ellipse, fill: fill.strong)
                    }
                    Label(title: tr! { "zh-Hans" => "头像", "en" => "Avatar" }, icon: "person.crop.circle", display: icon, style: title3)
                    if let Some(avatar) = picture {
                        Image(source: avatar, width: 48, height: 48, corner: 24, fit: cover, label: name)
                    }
                }
                Row(gap: 0) {
                    Column(gap: 4) {
                        if signed_in {
                            Text(content: name, style: headline, lines: 1)
                            Text(content: user, style: caption, emphasis: secondary, lines: 1)
                        } else {
                            if bangumi {
                                Text(content: tr! { "zh-Hans" => "Bangumi", "en" => "Bangumi" }, style: headline, lines: 1)
                            } else {
                                Text(content: tr! { "zh-Hans" => "弹弹play", "en" => "dandanplay" }, style: headline, lines: 1)
                            }
                            Text(content: tr! { "zh-Hans" => "尚未登录", "en" => "Not signed in" }, style: caption, emphasis: secondary, lines: 1)
                        }
                    }
                    Spacer
                }
                if signed_in {
                    if bangumi {
                        Button(id: bangumi_sign_out, label: tr! { "zh-Hans" => "退出登录", "en" => "Sign Out" }) => app.bangumi_sign_out()
                    } else {
                        Button(id: account_sign_out, label: tr! { "zh-Hans" => "退出登录", "en" => "Sign Out" }) => app.sign_out()
                    }
                } else {
                    if bangumi {
                        Button(id: bangumi_open, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, prominence: prominent, clears: draft.password) => app.open_bangumi_login(settings_window)
                    } else {
                        Button(id: account_open, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, prominence: prominent, clears: draft.password) => app.open_account_login(settings_window)
                    }
                }
            }
            if busy {
                Text(content: tr! { "zh-Hans" => "正在检查登录状态…", "en" => "Checking your session…" }, style: caption, emphasis: secondary)
            } else if "" != message {
                Text(content: message, style: caption, emphasis: secondary, lines: 3)
            }
        }
    }
}
