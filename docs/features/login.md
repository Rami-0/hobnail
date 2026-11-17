# Login and security

**Viewing is open** to anyone who can reach the port (publish it only on
addresses you trust: see [install.md](../install.md)). **Every change needs a
login**, and until a password exists the app is read-only.

Why: once the browser can write `config.toml`, anyone on the network could add
a `command` widget, which runs shell commands inside a container that has the
Docker socket — effectively root on the host. So writes are locked until you set
a password **on the host**:

```sh
docker exec -it hobnail /hobnail set-password      # asks twice, no echo
echo 'a long passphrase' | docker exec -i hobnail /hobnail set-password
```

There is no web "set the first password" form, so nobody can claim the instance
before you. Setting a password signs out every session. Minimum 8 characters.

**How it is enforced**

- One gate in the router: every request other than `GET`/`HEAD` must carry a
  valid session cookie, the `X-RH-CSRF` header and a same-origin `Origin`
  (or `Referer`). Only `POST /api/login` is exempt (it checks `Origin` itself).
  Tested end to end for every write route (`src/http/gate_tests.rs`).
- Requests whose `Host` header is not an IP address, `localhost`, the host
  name or a name in `RH_ALLOWED_HOSTS` are refused with 421 (DNS rebinding);
  `/healthz` is exempt.
- Password: PBKDF2-HMAC-SHA256, 600,000 iterations, random salt, stored only as
  a hash in the database.
- Session: 32 random bytes in an `HttpOnly; SameSite=Strict` cookie for 7 days;
  only its SHA-256 is stored; revocable (sign out, sign out others, password
  change).
- Brute force: per-IP and global back-off after failed logins (1 s, 2 s, 4 s …
  up to 60 s). Clients that already signed in since start skip the global part,
  so a stranger cannot lock you out.
- Redirect after login (`?next=`): only same-origin paths; control characters
  (including TAB), spaces, `//`, backslashes and other origins are refused, on
  the server and in the browser.
- `command` widgets can never be added, changed or enabled from the app; secrets
  are never shown, and a secret cannot be pointed at a new host from the app
  (see [settings.md](settings.md)).
- Login history (with IPs of failed attempts) is visible only when signed in.
- Settings forms never send a value in a URL: they post, and without
  JavaScript their save buttons are disabled (the write guard needs a header a
  plain form cannot send). A wrong current password in Settings › Security
  counts against the same back-off as a failed login.
- After signing in again in another tab, an open Settings page picks up the
  new session's CSRF token by reloading itself in the background (a same-origin
  GET of the page), so unsaved input survives an expired session.

**Not covered:** the cookie cannot be `Secure` over plain HTTP. Sign in over an
encrypted path (a VPN such as Tailscale, or a TLS reverse proxy), not over a
plain LAN address. Single user (`admin`); no roles.
