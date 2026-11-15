---
id: settings-hosts
title: Remote
summary: The Remote section — the machines you can reach and their SSH profiles, what runs there, this desktop serving others, and the drone list.
keywords: [remote, hosts, ssh, ssh profile, drone, drones, host mode, drone mode, stop drone, key file, password, passphrase, lifetime, persistent, tmux, terminal, drone list]
order: 70
status: current
related: [settings, remote-overview, remote-connect]
---

The **Remote** section of [Settings](settings) covers working on other machines, in three tabs.
**Remotes** lists every connection — saved hosts, SSH profiles and the drones running on them —
**Host mode** lets other Kubiq instances reach this one, and **Settings** holds the drone list URL.
Using them is in [Remote hosts](remote-overview).

Every list works the same way: its header says how many rows it has and carries **Add** at the
right; each row's actions are icons (hover one for its name); **Add** or a row's **Edit** opens the
editor in place of the list, with a back arrow and **Remotes › name** at the top and **Cancel**,
**Save** and **Delete** at the bottom.

## The parts

| Setting | What it changes for you |
|---|---|
| **Active host** | The machine a new pane or a new project lands on by default; it never moves one that already exists |
| **Saved hosts** | Every host you have connected to, with a chip (**attached**, **reconnecting**, **last attempt failed** or **saved**), **Disconnect** while attached, and **Forget** |
| **SSH profiles** | One row per target: **Test**, **Open terminal**, **Forget secret** for a filed passphrase or password, **Edit** and **Clone** |
| **Terminal**, **Remote drone**, **Persistent** | Under each profile: offer it as a terminal; offer it to carry a drone; keep what runs there alive when the link drops — the terminal reattaches a `tmux` (or `screen`) session, and a drone started with it waits for the **timeout** shown beside it |
| **Name**, **Method**, **Host**, **Port**, **User** | In the profile editor: what you call it, how it signs in (**Agent**, **Key file**, **Password** or **ssh config alias**), and where it goes |
| **Drones** | One row per saved host reached over SSH, with **Check** and **Refresh**, its drones under it with **Stop**, and **Edit tunnels** |
| **Host mode** | The endpoints this desktop serves while it runs — **HTTP**, with TLS and its tunnel allow-list, or a kind your edition adds — each with its switch, status and connect string |
| **Drone list URL** | Where a list of drones for SSH is published; saved, not yet read |

## What you can do here

- Reach a machine using a profile: [Connecting to a host](remote-connect).
- Work on it from start to finish: [Work on a remote machine](guides-remote-machine).
- Choose where a project's folder lives from then on: [Project settings](settings-project).

## When something goes wrong

- **A password or passphrase is not remembered.** It is stored in this machine's credential store and
  never shown again; leave the box blank in the editor to keep it, and use **Forget secret** to clear it.
- **Back asks to discard changes.** Something was typed since the editor opened; **Keep editing**
  returns to it, **Discard** leaves without saving.
- **Drones says no saved ssh hosts.** Connect to a host with the **Session** or **Managed** lifetime;
  it then appears here.
- **A drone row says version differs.** It was started by another build; **Stop** it, then connect
  again so a matching one is started.
- **Stopping a drone ended my panes.** **Stop** asks first, because every pane the drone holds ends
  with it.
- **A profile is missing from the connect dialog.** Its **Remote drone** box is unticked, or there is
  no profile yet — **Add** one here.
