# Player commands

These are the built-in commands every player can use. Type a command at the
prompt and press enter. Commands match on a **prefix**, so `n` is enough for
`north` and `inv` for `inventory`, as long as the prefix is unambiguous.

## Looking around

| Command | Aliases | What it does |
|---|---|---|
| `look` | `l` | Show the current room: title, description, obvious exits, the players here, and anything else here. |

## Seeing other players

Players in your room appear in `look` on their own line — `Alice is here.`,
or `Alice, Bob and Carol are here.` when several are present. Objects are
listed separately under "Also here:".

When a player logs in you'll see `Alice appears from nowhere.`; when they
quit or lose their connection you'll see `Alice disappears.`. Walking
between rooms is announced too (`Alice leaves north.` / `Alice arrives from
south.`).

## Moving

| Command | Aliases | What it does |
|---|---|---|
| `north` | `n` | Leave through the north exit. |
| `east` | `e` | Leave through the east exit. |
| `south` | `s` | Leave through the south exit. |
| `west` | `w` | Leave through the west exit. |
| `up` | `u` | Leave through the up exit. |
| `down` | `d` | Leave through the down exit. |

Moving shows you the room you arrive in. If there is no exit that way, you are
told so and stay put. Other players in the room you leave see you depart, and
those in the room you enter see you arrive.

## Talking

| Command | What it does |
|---|---|
| `say <message>` | Speak aloud. |

Everyone else in your room hears what you say. Your message is capped at 4 KiB.
Terminal control codes you type are stripped, and any color markup is shown
**literally** — you cannot inject styling or escape sequences into what
others see.

## Who's around and leaving

| Command | What it does |
|---|---|
| `who` | List the players currently online. |
| `quit` | Leave the game and disconnect. |

## Items

| Command | Aliases | What it does |
|---|---|---|
| `get <object>` | `take` | Pick an item up off the floor. |
| `drop <object>` | | Put an item you are carrying down. |
| `inventory` | `i`, `inv` | List what you are carrying. |

### Picking the right object

When more than one nearby object matches what you typed, you can disambiguate:

- **By number** — `get sword.2` takes the *second* matching sword.
- **All of them** — `get all coin` takes every matching coin.
- Otherwise the game lists the matches with numbers and asks which you meant.
  Just type the command again, more specifically — nothing is held waiting on
  your answer.

Object names match on a prefix and ignore case, so `get gob` finds the goblin's
loot bag if `gob` is unambiguous.
