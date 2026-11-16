use super::{
    App, Msg,
    backend::{self, AuthChange, Event},
    state::{LoginMethod, LoginPhase},
    theme::*,
};
use iced::widget::{button, column, container, image, row, scrollable, space};
use iced::{Element, Fill};
use zeroize::{Zeroize, Zeroizing};

fn account_status(has_key: bool) -> &'static str {
    if has_key {
        "已保存自动登录凭据，启动时验证"
    } else {
        "未保存自动登录凭据，请重新添加账号"
    }
}
impl App {
    pub(super) fn accounts_page(&self) -> Element<'_, Msg> {
        let enabled = !self.critical_busy() && !self.modal_open();
        let default = self.auth.default_account().map(|a| a.snda_id.as_str());
        let mut body = column![
            row![
                heading("账号管理", 20),
                space().width(Fill),
                btn("添加账号", enabled.then_some(Msg::AddAccount), true)
            ]
            .align_y(iced::Alignment::Center),
            text("管理本机保存的账号。启动游戏时会使用已有凭据自动登录。")
                .size(13)
                .color(MUTED)
        ]
        .spacing(16);
        if self.auth.accounts.is_empty() {
            body = body.push(section(
                "还没有保存的账号",
                text("点击「添加账号」开始登录。").color(MUTED),
            ));
        }
        for pair in self.auth.accounts.chunks(2) {
            let mut cards = row![].spacing(16);
            for account in pair {
                let mut content = column![heading(account.display_name(), 16)].spacing(6);
                if account.username.is_some() {
                    content = content.push(
                        text(format!("SDO ID：{}", account.snda_id))
                            .size(12)
                            .color(MUTED),
                    );
                }
                if let Some(area) = &account.area {
                    let label = self
                        .areas
                        .iter()
                        .find(|a| a.area_id == *area || a.area_name == *area)
                        .map(|a| a.area_name.as_str())
                        .unwrap_or(area);
                    content =
                        content.push(text(format!("默认大区：{label}")).size(12).color(MUTED));
                }
                let is_default = default == Some(account.snda_id.as_str());
                content = content
                    .push(
                        text(account_status(account.can_auto_login()))
                            .size(12)
                            .color(MUTED),
                    )
                    .push(
                        row![
                            btn(
                                if is_default { "默认" } else { "设为默认" },
                                (enabled && !is_default)
                                    .then(|| Msg::Default(account.snda_id.clone())),
                                false
                            ),
                            button(text("删除").size(12))
                                .padding([8, 12])
                                .on_press_maybe(
                                    enabled.then(|| Msg::Delete(account.snda_id.clone()))
                                )
                                .style(danger)
                        ]
                        .spacing(8),
                    );
                cards = cards.push(card(content));
            }
            body = body.push(cards);
        }
        body.into()
    }
    pub(super) fn persist_auth(
        &mut self,
        change: AuthChange,
        login: Option<u64>,
        selected: Option<String>,
    ) {
        if self.critical_busy() {
            self.status = "请等待当前任务完成后修改账号".into();
            return;
        }
        self.auth_busy = true;
        backend::save_auth(&self.runtime, self.bus.clone(), change, login, selected);
    }
    pub(super) fn persist_login(&mut self) {
        let Some(token) = self.login.pending_token.clone() else {
            return;
        };
        if self.critical_busy() {
            self.login.error = Some("请等待当前任务完成后重试保存账号".into());
            self.login.phase = LoginPhase::Failed;
            return;
        }
        self.login.phase = LoginPhase::Saving;
        self.login.error = None;
        let area = self.selected_area().map(|a| a.area_name);
        let selected = Some(token.snda_id.clone());
        self.persist_auth(
            AuthChange::Add(token, area),
            Some(self.login.task.generation),
            selected,
        );
    }
    pub(super) fn start_login(&mut self) {
        if self.login.phase == LoginPhase::Saving {
            return;
        }
        self.login.task.cancel();
        self.login.error = None;
        self.login.qr = None;
        self.login.pending_token = None;
        self.login.serial = None;
        let Some(launcher) = self.launcher.clone() else {
            self.login.phase = LoginPhase::Idle;
            return;
        };
        let account = self.login.username.trim().to_string();
        if self.login.method != LoginMethod::Qr && account.is_empty() {
            self.login.error = Some("请输入账号".into());
            self.login.phase = LoginPhase::Failed;
            return;
        }
        if self.login.method == LoginMethod::Password && self.login.password.is_empty() {
            self.login.error = Some("请输入密码".into());
            self.login.phase = LoginPhase::Failed;
            return;
        }
        let method = self.login.method;
        let password = Zeroizing::new(std::mem::take(&mut self.login.password));
        let generation = self.login.task.generation;
        let bus = self.bus.clone();
        self.login.phase = LoginPhase::Requesting;
        self.login.task.handle = Some(self.runtime.spawn(async move {
            let result = match method {
                LoginMethod::Qr => match launcher.request_qr_code().await {
                    Ok(session) => {
                        bus.send(Event::Qr(generation, session.image_data().to_vec()));
                        session.wait_for_scan(None).await
                    }
                    Err(e) => Err(e),
                },
                LoginMethod::Push => match launcher.request_push_login(&account).await {
                    Ok(session) => {
                        bus.send(Event::Push(
                            generation,
                            session.serial_num().map(str::to_owned),
                        ));
                        session.wait_for_confirm(None).await
                    }
                    Err(e) => Err(e),
                },
                LoginMethod::Password => launcher.login_password(&account, &password).await,
            };
            bus.send(Event::Login(generation, result.map_err(|e| e.to_string())));
        }));
    }
    pub(super) fn login_view(&self) -> Element<'_, Msg> {
        let saving = self.login.phase == LoginPhase::Saving;
        let busy = matches!(
            self.login.phase,
            LoginPhase::Requesting | LoginPhase::Waiting
        );
        let selected = match self.login.method {
            LoginMethod::Qr => 0,
            LoginMethod::Push => 1,
            LoginMethod::Password => 2,
        };
        let mut content = column![
            row![
                heading("添加账号", 18),
                space().width(Fill),
                btn("关闭", (!saving).then_some(Msg::CloseLogin), false)
            ]
            .align_y(iced::Alignment::Center),
            segments(
                "method",
                &["叨鱼扫码登录", "叨鱼推送登录", "密码登录"],
                selected,
                !saving
            )
        ]
        .spacing(20);
        if let Some(error) = &self.login.error {
            content = content.push(text(error).size(13).color(RED));
        }
        if self.login.pending_token.is_some() {
            content = content
                .push(
                    text(if saving {
                        "登录成功，正在保存账号…"
                    } else {
                        "登录成功，但保存失败。修复文件权限后可重试，无需重新扫码。"
                    })
                    .color(MUTED),
                )
                .push(btn(
                    "重试保存账号",
                    (!saving).then_some(Msg::RetrySave),
                    true,
                ));
        } else if self.login.method == LoginMethod::Qr {
            if let Some(qr) = &self.login.qr {
                content = content
                    .push(container(image(qr.clone()).width(220).height(220)).center_x(Fill))
                    .push(
                        text("使用叨鱼 App 扫码并确认，5 分钟内有效。")
                            .size(13)
                            .color(MUTED),
                    );
            } else {
                content = content.push(
                    text(if self.launcher.is_none() {
                        "正在初始化登录服务…"
                    } else if busy {
                        "正在获取二维码…"
                    } else {
                        "请刷新二维码重试"
                    })
                    .color(MUTED),
                );
            }
            content = content.push(btn(
                "刷新二维码",
                (self.launcher.is_some() && self.login.phase != LoginPhase::Requesting)
                    .then_some(Msg::Login),
                false,
            ));
        } else {
            content = content.push(
                column![
                    text("账号").size(13),
                    input("username", &self.login.username, "", !busy && !saving)
                ]
                .spacing(6),
            );
            if self.login.method == LoginMethod::Password {
                content = content.push(
                    column![
                        text("密码").size(13),
                        input("password", &self.login.password, "", !busy && !saving)
                    ]
                    .spacing(6),
                );
            }
            if self.login.phase == LoginPhase::Waiting && self.login.method == LoginMethod::Push {
                if let Some(serial) = &self.login.serial {
                    content = content.push(heading(format!("验证序号：{serial}"), 20));
                }
                content = content.push(
                    text("请在叨鱼 App 中核对序号并确认，30 秒内有效。")
                        .size(13)
                        .color(MUTED),
                );
            }
            if busy {
                content = content
                    .push(
                        text(if self.login.method == LoginMethod::Push {
                            "正在等待推送确认…"
                        } else {
                            "正在登录…"
                        })
                        .color(MUTED),
                    )
                    .push(btn("取消本次登录", Some(Msg::CancelLogin), false));
            } else {
                content = content.push(btn(
                    if self.login.method == LoginMethod::Push {
                        "发送推送"
                    } else {
                        "登录并添加"
                    },
                    self.launcher.is_some().then_some(Msg::Login),
                    true,
                ));
            }
            if self.login.method == LoginMethod::Password {
                content = content.push(
                    text("密码只用于本次登录，不会保存到本机。")
                        .size(12)
                        .color(MUTED),
                );
            }
        }
        card(scrollable(content).height(iced::Length::Shrink))
            .max_width(440)
            .into()
    }
    pub(super) fn delete_view(&self) -> Element<'_, Msg> {
        card(
            column![
                heading("移除本机账号？", 18),
                text("只移除本机记录，不会注销盛趣账号。").color(MUTED),
                row![
                    btn("取消", Some(Msg::Escape), false),
                    btn(
                        "确认删除",
                        (!self.critical_busy()).then_some(Msg::ConfirmDelete),
                        true
                    )
                ]
                .spacing(8)
            ]
            .spacing(20),
        )
        .max_width(400)
        .into()
    }
    pub(super) fn change_login_method(&mut self, n: usize) {
        if self.login.phase == LoginPhase::Saving {
            return;
        }
        self.login.reset();
        self.login.method = match n {
            0 => LoginMethod::Qr,
            1 => LoginMethod::Push,
            _ => LoginMethod::Password,
        };
        if self.login.method == LoginMethod::Qr {
            self.start_login();
        }
    }
    pub(super) fn password_input(&mut self, mut value: String) {
        self.login.password.zeroize();
        std::mem::swap(&mut self.login.password, &mut value);
        value.zeroize();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_not_presented_as_verified_session() {
        assert_eq!(account_status(true), "已保存自动登录凭据，启动时验证");
        assert_eq!(account_status(false), "未保存自动登录凭据，请重新添加账号");
    }
}
