# Getting started

This page walks you through everything that happens between the moment you
connect and the moment you're standing in the world, ready to play.

## Connecting

When you connect, the game greets you with a **welcome banner** (its exact
wording is set per-server) followed by a prompt telling you how to log in or
register. From this prompt you have a small set of commands available:

| Command | What it does |
|---|---|
| `login <name>` | Log in to an existing account. |
| `register <name>` | Create a new account. |
| `who` | List connected players. *(Currently a stub — always empty.)* |
| `help` / `?` | List the commands available at this prompt. |
| `quit` | Disconnect. |

## Logging in

Type `login <name>`. You'll be asked for your password:

```
> login aria
Password:
```

Type your password and press enter. If the name or password doesn't match,
you'll get a generic "login failed" message — the game deliberately doesn't
say whether the name exists, so no one can use failed logins to fish for
valid account names.

!!! note
    The server asks your client to stop echoing while you type a password
    (telnet echo suppression, RFC 857). Most MUD clients and plain `telnet`
    honor it; if yours refuses, it will still display the password as you
    type — be mindful of who can see your screen.

## Registering

Type `register <name>` to create a new account. Names may use letters,
digits, and `_ ' -`, and must be between 1 and 32 characters. You'll be
prompted twice, to catch typos:

```
> register aria
Password:
Confirm password:
```

If the two entries don't match, you're returned to the connection prompt —
type `register <name>` again to retry. If the name is already taken, you're
told so and can pick another the same way.

## Choosing your character

Once you're logged in (whether you just registered or logged into an
existing account), you move to **character selection** — picking which
character you want to play:

| Command | What it does |
|---|---|
| `play <name>` | Enter the world as an existing character, by name. |
| `play <number>` | Enter the world as an existing character, by its position in your character list. |
| `new <name>` | Create a new character and enter the world as it. |

A brand-new account has no characters yet, so it's prompted straight to
`new <name>` to create its first one. Character names follow the same rules
as account names (letters, digits, `_ ' -`, 1–32 characters).

Once you've entered the world, you're in-world play — see
[Player commands](commands.md) for what you can do from there.
