//! `/login`: the password form. With JavaScript the script posts
//! `{"password": ...}` to `POST /api/login`, reads `{"ok":true,"csrf":...}`
//! and then goes to `?next=` (checked again in the script, see `app.js`
//! `safeNext`). Without JavaScript the form posts normally and the server
//! answers with a redirect itself.

use maud::html;

use crate::ui::fmt::{login_next, parse_query, query_get};
use crate::ui::ic;
use crate::ui::pages::crumb_title;
use crate::ui::shell::{layout, Ctx, Nav};

pub const SET_PASSWORD_CMD: &str = "docker exec -it hobnail /hobnail set-password";

pub fn page(ctx: &Ctx) -> String {
    let q = parse_query(ctx.query);
    let next = query_get(&q, "next").and_then(login_next).unwrap_or("/");
    let set = crate::auth::password_is_set(ctx.state);
    let body = html! {
        div.page.narrow {
            div.panel.primary.login {
                div.login-h {
                    (ic("lock"))
                    h2 { "Sign in to Hobnail" }
                }
                p.muted { "Viewing is open to everyone who can reach this page. Signing in is only needed to change settings." }
                @if let Some(u) = &ctx.user {
                    div.banner.ok {
                        (ic("check"))
                        div { "Signed in as " strong { (u) } ". " a href=(next) { "Continue" } " or " button.linklike type="button" data-act="logout" { "sign out" } "." }
                    }
                }
                @if !set {
                    div.banner.warn #nopw {
                        (ic("info"))
                        div {
                            strong { "No password is set yet." }
                            " Nothing can be changed from the browser until the owner sets one on the host:"
                            pre.cmdline { code { (SET_PASSWORD_CMD) } }
                        }
                    }
                }
                form #login method="post" action="/api/login" autocomplete="on" {
                    input type="hidden" name="next" value=(next);
                    label for="pw" { "Password" }
                    input #pw type="password" name="password" autocomplete="current-password" required
                        autofocus[set] aria-describedby="login-msg" disabled[!set];
                    p.form-msg #login-msg role="alert" aria-live="assertive" {}
                    button.btn.primary type="submit" disabled[!set] { (ic("log-in")) "Sign in" }
                }
            }
        }
    };
    layout(ctx, Nav::None, "Sign in", crumb_title("Sign in"), body)
}
