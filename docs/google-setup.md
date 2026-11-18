# Setting up Google Drive backup

TuxNotes backs up to the user's own Google Drive. To do that it needs an **OAuth client** from Google Cloud. This is a public identifier for the app, not a password.

You only need this guide if you:

- **maintain TuxNotes releases**: you create one client, and release builds include it so users just click "Sign in with Google", or
- **build TuxNotes yourself**: you can use your own client.

## 1. Create a Google Cloud project

1. Open <https://console.cloud.google.com/> and create a project (for example "TuxNotes").
2. Go to **APIs & Services → Library**, search for **Google Drive API**, and click **Enable**.

## 2. Configure the consent screen

1. Go to **APIs & Services → OAuth consent screen** (called "Google Auth Platform" in newer consoles).
2. **User type:** External.
3. **App name:** TuxNotes. Add a support email, and optionally the logo from `src-tauri/icons/icon.png`.
4. **Scopes:** add only `https://www.googleapis.com/auth/drive.file`.
   - This scope only lets TuxNotes see files it created itself. Google classifies it as *non-sensitive*, so publishing doesn't need a security assessment.
5. **Test users:** while the app is in *Testing* mode, add the Google accounts that may sign in (up to 100).

## 3. Create the OAuth client

1. Go to **APIs & Services → Credentials → Create credentials → OAuth client ID**.
2. **Application type:** **Desktop app**. This matters: TuxNotes uses the loopback redirect (`http://127.0.0.1:<random port>`) that only Desktop clients allow.
3. Copy the **Client ID** and **Client secret**.

For Desktop apps Google says the client secret is not treated as confidential, because anyone can extract it from an installed app. Still, don't commit it to the repository. Use one of the options below.

## 4. Give the client to TuxNotes

Pick one. They are listed in priority order; the first one found wins.

| Option | How | Good for |
|---|---|---|
| In the app | Backup → **Advanced** → paste the Client ID and Secret → Save | Personal builds, testing |
| Environment variables at runtime | `TUXNOTES_GOOGLE_CLIENT_ID=… TUXNOTES_GOOGLE_CLIENT_SECRET=… npm run tauri dev` | Development |
| Built into the binary | Set the same two variables **when building** (`npx tauri build`) | Official releases |

### Official releases (GitHub Actions)

1. In the GitHub repository, go to **Settings → Secrets and variables → Actions** and add:
   - `TUXNOTES_GOOGLE_CLIENT_ID`
   - `TUXNOTES_GOOGLE_CLIENT_SECRET`
2. In `.github/workflows/release.yml`, uncomment the two lines that pass them to the build.

## 5. Publishing (removing the 100-user limit)

While the consent screen is in **Testing** mode:

- only listed test users can sign in, and
- **sign-ins expire after 7 days**, so users have to sign in again every week.

To make backups work for everyone, click **Publish app** on the consent screen. Because TuxNotes only uses `drive.file`, Google requires only **brand verification**:

- a homepage (the GitHub repo or a GitHub Pages site),
- a **privacy policy** URL (a short page saying notes go only to the user's own Drive and nowhere else),
- a verified domain for those pages (GitHub Pages under your own domain works).

Verification usually takes a few days.

## How it works (for reviewers)

- **Sign-in:** the authorization code flow with PKCE (S256). TuxNotes opens the system browser and receives the redirect on a temporary `127.0.0.1` listener. Nothing is embedded in the app.
- **Tokens:** the refresh token is stored in the desktop keyring (Secret Service: GNOME Keyring, KWallet). If no keyring is available, it goes in `~/.local/share/io.github.tuxnotes.TuxNotes/google-token` with mode `0600`. Access tokens stay in memory only.
- **Backups:** zip files (`manifest.json` plus a SQLite snapshot with a SHA-256 checksum), uploaded to a **TuxNotes Backups** folder in the user's Drive. Old backups beyond the "Keep" setting are deleted.
- **Restore:** the backup is downloaded, checked against its checksum, opened, and run through SQLite's integrity check, all **before** anything is replaced. A local copy of the current notes is saved to `~/.local/share/io.github.tuxnotes.TuxNotes/safety/` first (the last 5 are kept).
- **Disconnect:** the token is revoked with Google and deleted locally. Backups already on Drive stay there.
