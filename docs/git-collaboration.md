# Git collaboration

[Documentation](README.md)

Wirebolt can show status and explicitly commit, pull or push a workspace. It does not configure hosting credentials or synchronize in the background.

## Prepare the workspace

The folder containing `wirebolt.toml` must be the **root** of its Git repository. A workspace inside a subfolder of a larger repository is not supported: Git Collaboration explains this and offers **Show in Finder**. Open the repository root as the workspace, or move the workspace into its own repository.

If the workspace folder is not a repository yet, Git Collaboration shows **Initialize Git Repository**, which runs `git init` in the workspace folder. It does not initialize a folder that is already inside another repository. Then add a remote and configure Git identity and authentication with your normal Git tools:

```sh
cd /path/to/your/workspace
git remote add origin <your-repository-url>
```

Replace the path and remote placeholder with your own values. You can also clone an existing workspace repository and open the folder containing `wirebolt.toml`. The built-in workspace in Application Support works too, but a folder you choose with **File → New Workspace…** is easier to find and share.

## Review and share

1. Save request edits with **⌘S**.
2. Open **Workspace → Git Collaboration…** (**⌃⌘G**).
3. Review the branch, upstream and changed files.
4. Enter a commit message and click **Commit**.
5. Click **Push** when ready to publish to the configured remote. A branch without an upstream shows **Publish**, which pushes it to `origin` and tracks it. Push is disabled, with the reason in its tooltip, when there is nothing to push.

![Git collaboration sheet for a disposable demo repository](assets/screenshots/git.png)

Wirebolt commits its managed workspace documents. Unrelated files are not included by that action. Review ordinary literal fields for credentials before sharing; secret references do not make all user-entered text safe automatically.

## Pull and conflicts

Save or discard request edits before pulling. Pull requires an upstream; first configure a remote and establish tracking, using Git if necessary. A first push can establish an upstream when the remote is configured.

When a pull stops with conflicts, Git Collaboration lists the conflicted files and disables Commit, Pull and Push until the merge is finished or aborted. Wirebolt does not silently choose a side or automatically resolve conflicts. You can:

- Click **Abort Merge…** to run `git merge --abort`. The workspace files return to your last commit and Wirebolt reloads them. You can pull again later.
- Click **Show in Finder** to select the conflicted files, resolve them in a text editor or Git tool, finish the merge there (`git add` and `git commit`), then reopen the workspace.

Git authentication failures come from your Git/remote setup, which is separate from request authentication and API proxy settings.
