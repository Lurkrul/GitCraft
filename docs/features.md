# Features to check

Every feature GitCraft has, written as something to try in the running app. Most were checked as
they were built. This list is for a full pass at the end (and before a release). Items marked
**(not checked yet)** were skipped when they were built.

Each step adds its items here. Tick the boxes on the final pass. Linux (Fedora, Wayland) items are
at the end.

Tip: set `GITCRAFT_CONFIG_DIR` to a scratch folder to start with fresh settings without touching
your own.

## Window and settings

- [ ] The window title reads "GitCraft", and "GitCraft — <path>" while a repository is open.
- [ ] Closing the window (the X button, or File → Exit) quits cleanly.
- [ ] Resize the window and restart: it opens at the same size.
- [ ] Maximize and restart: it opens maximized. Un-maximizing returns to the earlier size.
- [ ] A damaged `config.toml` (junk text, or a size like `width = "abc"`) gives a message in the status
      bar, and the app falls back to defaults instead of crashing.
- [ ] Copy and paste work in text boxes (Ctrl+C / Ctrl+V).

## Menus, tabs and shortcuts

- [ ] File → Open Repository… (and Ctrl+O) opens the folder picker.
- [ ] Help → About GitCraft opens the About window, and Close dismisses it.
- [ ] Main tabs: Repository, Issues, Projects. Issues and Projects are placeholders.
- [ ] Shortcuts do nothing while a menu, dialog or the About window is open.

## Layout (Repository tab)

- [ ] Toolbar, like GitHub Desktop: three big boxes side by side, each with a label on top and the value
      larger below (both easy to read against the box): "Current repository", "Current branch" (both with a ▾ arrow), then
      the Push/Pull box ("Push" / "Push 2 commits", "Pull" / "Pull 3 commits", "Fetch origin" /
      "Up to date", "Publish branch" / "Not on origin yet").
- [ ] Clicking the repository or branch box opens its list below it; picking an item closes it; a
      click outside closes it. Long names are cut with "…" and the boxes keep their size.
- [ ] The left pane has two tabs at its top: Changes and History. Changes shows the changed files,
      the commit box and the diff; History shows the commit list, details and diffs.
- [ ] Changes and History share the left pane's width: drag the divider in one, switch, and the
      other has the same width (also after a restart).
- [ ] GitCraft opens on Changes the first time, then on whichever view you left it in.
- [ ] The history filter ("Show:") appears next to the boxes only in the History view.

## Opening a repository

- [ ] Opening a repository through the folder picker shows its history, with a commit count in the
      status bar.
- [ ] Opening a folder that isn't a repository shows an error, and the current repository stays on
      screen.
- [ ] A spinner shows in the status bar while loading.
- [ ] A repository with more than 10,000 commits loads the first 10,000 and says so in the status bar.
- [ ] Restart: the last repository reopens, with its branch and divider positions.
- [ ] Rename or delete the last repository's folder, then restart: the status bar says why, and the
      welcome page shows.
- [ ] File → Open Recent lists repositories opened before, newest first, with the open one checked.
      Picking another opens it on its saved branch.
- [ ] The "Current repository" box lists the same repositories (full path on hover), and
      "Open other repository…" opens the folder picker.
- [ ] A recent repository whose folder was deleted or renamed shows greyed out ("not found" in the
      menu) after switching back to GitCraft.
- [ ] File → Open Recent → Clear recent list leaves only the open repository.
- [ ] With nothing remembered, Open Recent shows a greyed-out "No recent repositories".
- [ ] A saved branch that has since been deleted is forgotten, and you get "All branches".

## History view: history and graph

- [ ] "Show:" filter: All branches, then Local (current branch marked), then Remote.
- [ ] Choosing a branch shows only its history. "All branches" shows everything.
- [ ] The graph draws lanes, with curved lines for merges and forks, a dot per commit and a smaller dot
      for merges.
- [ ] Ref labels: the checked-out branch first (bold), local branches filled, remote branches outlined,
      tags grey, and a "HEAD" label when detached.
- [ ] An initials avatar before each summary. The same author always gets the same colour.
- [ ] Click a row to select it. Up/Down move the selection and keep it in view.
- [ ] Switching branch keeps the selection if that commit is still shown.

## History view: details and diffs

- [ ] The divider between the history and the details can be dragged (or focused and moved with the
      arrow keys).
- [ ] Details panel: summary, author avatar, short hash, and up to 3 lines of description with
      "More info".
- [ ] More info: full description, date, committer (only if different), selectable full hash, and
      parent links. Clicking a parent selects it.
- [ ] The More info choice is remembered across commits and restarts.
- [ ] Changed files list coloured status letters and +/- counts. Binary files show no counts.
- [ ] A merge commit shows its changes against the first parent.
- [ ] The first file's diff shows automatically. Clicking another file shows its diff.
- [ ] Diff view: old and new line numbers, green and red lines, hunk headers, horizontal scrolling for
      long lines.
- [ ] A new file shows all green, a deleted file all red.
- [ ] The divider between details and diff can be dragged, and both dividers survive a restart.

## Branches

- [ ] The "Current branch" box shows the checked-out branch; its list has "New branch…" and the local
      branches.
- [ ] Clicking another branch in the list checks it out; uncommitted changes that
      don't conflict come along.
- [ ] A checkout that would overwrite local changes is refused with git's message naming the files,
      and nothing changes.
- [ ] New branch… (in the list, Branch menu, or Ctrl+Shift+N) asks for a name; "Check out the new
      branch" is ticked by default. Spaces are caught as you type; an invalid name (`a..b`) shows git's
      rules in the dialog, which stays open.
- [ ] Right-click a branch → New branch from here… starts the new branch at that branch.
- [ ] Rename… (right-click, or Branch → Rename Current Branch…) renames; if the history filter shows
      that branch, it follows the new name (also after a restart).
- [ ] Delete… asks first. A branch whose commits are all elsewhere says none are lost; one with
      unmerged commits names how many ("Delete branch and 2 commits"). The checked-out branch can't be
      deleted.
- [ ] After a commit or a branch change, the selected commit in the history stays selected.

## Fetch, pull and push

- [ ] The Push/Pull box shows what fits: "Fetch origin" / "Up to date", "Pull" / "Pull 3 commits"
      when the remote has new commits, "Push" / "Push 2 commits" when you have unpushed commits,
      "Publish branch" / "Not on origin yet" for a branch never pushed. Hovering says what it does.
- [ ] A repository with no remote shows "No remote" (disabled, with the reason on hover).
- [ ] Fetch (button or Ctrl+Shift+T) picks up commits pushed elsewhere: the button turns into Pull.
- [ ] Pull (Ctrl+Shift+P) brings them in; the history and files update. It follows your git config
      (merge, rebase or fast-forward only).
- [ ] Push (Ctrl+P) sends your commits; on a new branch it publishes it (sets the upstream).
- [ ] A push the remote refuses (it has commits you don't) says to pull first.
- [ ] Repository → Force Push… asks first and names how many remote commits would be replaced. If the
      remote changed since your last fetch, it is refused and nothing is overwritten.
- [ ] While a network command runs: the box shows it with a spinner, and clicking it cancels; the rest of the app
      keeps working; a second network command is refused; committing waits while a pull runs.
- [ ] Cancel stops it ("Push cancelled.").
- [ ] Pushing over HTTPS with Git Credential Manager shows its sign-in window when needed.

## Changes view: changes

- [ ] The changed files list a checkbox, the path from the repository root, and the change letter on
      the right.
- [ ] The change letter has room on its right, and a scroll bar doesn't cover it (long list).
      **(new; check now)**
- [ ] The first file is selected and its diff shown. The divider can be dragged and survives a
      restart.
- [ ] A modified, a deleted and an untracked file each show the right diff.
- [ ] The list refreshes on F5, when the Changes view is shown, and when the window gets the focus back.

## Changes view: committing

- [ ] Every file starts ticked. The all/none box turns part-filled when the files are mixed.
- [ ] Unticking a file and committing commits only the ticked files. The others stay, ticked again.
- [ ] Ticks survive F5 and switching away from the window and back.
- [ ] Staging in a terminal (`git add`, `git add -p`, `git mv`) doesn't change the ticks: new changes
      start ticked, and Commit takes exactly what is ticked.
- [ ] With Summary empty, the Commit button is disabled and hovering it says why.
- [ ] Ctrl+Enter in Summary or Description commits.
- [ ] A successful commit clears the message box and reloads the repository.
- [ ] Amend last commit fills in the last message (unless one is already typed). **(not checked yet)**
- [ ] Amending a commit that is already on a remote asks first ("Amend anyway"). **(not checked yet)**
- [ ] A failing pre-commit hook (`exit 1`) shows git's output under the button and keeps the message.
      **(not checked yet)**

## Changes view: picking lines

- [ ] Each added or removed line in the Changes view's diff has a checkbox; context lines don't. The
      History view's diff has no checkboxes.
- [ ] Unticking a line makes the file's box in the list part-filled; hovering it says only the
      ticked lines will be committed.
- [ ] Each hunk header has a box: part-filled when some of its lines are ticked. Clicking it unticks
      a fully ticked hunk, and ticks anything else fully.
- [ ] Shift+click a line ticks or unticks every changed line from the last clicked one.
- [ ] Each block of changes (added/removed lines with no unchanged line between) has its own checkbox,
      in the far-left column on the block's first line. Hovering it highlights the block; clicking
      unticks a fully ticked block, and ticks anything else fully. Other blocks are unaffected.
- [ ] Ticking every line again (or clicking the file's box) makes the file fully ticked.
- [ ] Ticking one line of an unticked file ticks just that line.
- [ ] Commit with some lines unticked: only the ticked lines are in the commit (check the Repository
      tab), the file on disk is unchanged, and the rest still shows as a change.
- [ ] Edit the file elsewhere (another part of it) and switch back: lines you unticked stay unticked.
- [ ] A file whose old last line had no newline: ticking only the new line after it gives a clear
      message under the Commit button, and nothing is committed.

## Changes view: discarding lines

- [ ] Right-click a line's checkbox in the diff: "Discard added line" (or "Discard removed line").
      Right-click a block's or hunk's checkbox: "Discard lines".
- [ ] Choosing it asks first, saying how many added and removed lines will be lost; Cancel or Esc
      changes nothing.
- [ ] Confirming reverts only those lines: other changes in the file stay, and the list and diff
      refresh. A copy of the file before the discard is in the Recycle Bin.
- [ ] Discarding a removed line puts it back; discarding an added line takes it out.
- [ ] Works on a new (untracked) file, and on a file with Windows (CRLF) line endings.
- [ ] A deleted file's boxes offer no discard (use "Discard changes…" on the file).

## Changes view: pre-staging (ticks saved)

- [ ] Untick a file, close GitCraft and open it again: the file is still unticked.
- [ ] Untick some lines of a file, restart: the same lines are unticked (the file's box part-filled).
- [ ] After a restart, commit straight away without opening a partly ticked file's diff: only its
      ticked lines are committed.
- [ ] A change made while GitCraft was closed (a new file, or a new edit) starts ticked.
- [ ] After committing part of a file, what is left of it is unticked, and stays so after a restart.
- [ ] `git status` / `git diff --cached` in a terminal show nothing staged by unticking: the ticks
      live in `.git/gitcraft/prestage`, not in git's index.
- [ ] Put junk in `.git/gitcraft/prestage` and reopen the repository: the status bar says the saved
      ticks were damaged, the file is renamed to `prestage.damaged`, and everything starts ticked.
- [ ] Ticking or unticking doesn't flash "Loading…" in the status bar.

## Changes view: right-click menu

- [ ] Discard changes… asks first and names exactly what will be lost. Esc or a click outside cancels.
- [ ] Discarding moves the current file to the Recycle Bin, then restores the committed version.
      **(not checked yet)**
- [ ] Discarding a new file only moves it to the Recycle Bin. **(not checked yet)**
- [ ] Ignore file adds `/path` to `.gitignore` (creating it if needed), and doing it twice doesn't add
      a duplicate line. **(not checked yet)**
- [ ] Ignore all .ext files adds `*.ext`. **(not checked yet)**
- [ ] Ignore is greyed out for tracked files.
- [ ] Copy path and Copy relative path put the right text on the clipboard. **(not checked yet)**
- [ ] Show in Explorer opens Explorer with the file selected. **(not checked yet)**
- [ ] Open uses the default program. **(not checked yet)**
- [ ] Open with ▸ lists remembered programs, Choose a program…, and the system "Open with" dialog.
- [ ] Choose a program… opens the file in that program and remembers it in the submenu.
      **(not checked yet)**
- [ ] The system "Open with" dialog opens. **(not checked yet)**

## Linux (Fedora, Wayland)

None of these have been checked yet.

- [ ] The window opens under Wayland, and closing works.
- [ ] Window size restore (the compositor may ignore it).
- [ ] The last repository, branch and dividers reopen after a restart (`~/.config/gitcraft/config.toml`).
- [ ] Changes view: status and diffs, including file names containing `:` or a backslash.
- [ ] Changes view refreshes when the window gets the focus back.
- [ ] Right-click menu: Discard moves the file to the Trash; Show in file manager and Open use
      `xdg-open`; Choose a program works.
