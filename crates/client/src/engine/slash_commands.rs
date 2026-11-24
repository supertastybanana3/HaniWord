// 2026-10-13: 行頭の / のあとの語を英字のまま渡すか (prefix_direct)。/ のあとの日本語がいつまでも英字のまま渡されないように、
// いろいろなアプリの / コマンドを調べた一覧に当たる語だけを英字のままにし、一覧の語と日本語の読みがぶつかる綴りは学習で見分ける。
//
// 英字のまま: 知っているコマンドの頭 (下の一覧・%APPDATA%\HaniWord\slash_commands.txt・学習した語) か、英単語の頭か、ローマ字で読めない並び (/mnt /usr)。
// それ以外で日本語として読めるようになったら、渡した文字を消して日本語の入力に戻す。
// 学習: 日本語に戻したあと、利用者がその書きかけを英字に直して確定したら (F9・F10・候補の一覧の英字で確定した、または消して英数モードで
// 同じ綴りを打った)、確定した綴りを覚える。2026-11-24: 学習のきっかけを確定にそろえた (前は書きかけを消しただけで覚えていた)。
// 消しただけでは覚えない。覚えてはいけない欄 (direct_field::private_field) では覚えない

use std::sync::Mutex;

use super::auto_english;

/// 知っている / コマンド (先頭の / を除く、小文字)。出典は slash_commands_sources.md
const BUILTIN: &[&str] = &[
    "a", "ability", "about", "absent", "accept", "achievements", "action", "add", "add-dir", "add_child",
    "add_contacts", "add_email", "addfriend", "admin", "advancement", "advisor", "af", "afk", "after", "agent",
    "agent-review", "agents", "agree", "aimassist", "alias", "all", "allchat", "allcmds", "allcommands", "alldeath",
    "allow-all", "allow-all-tools", "allowed-tools", "allowlist", "allpvpdeath", "ally", "allychat", "alwaysday",
    "amaze", "android", "angry", "announce", "api", "apologize", "app", "applaud", "applause", "apply-worktree",
    "approve", "apps", "architect", "archive", "arm", "artifact-capabilities", "artifact-diagramming", "artifacts",
    "ask", "asks", "assign", "assign_reviewer", "assist", "attach-files", "attach-folder", "attacktarget",
    "attribute", "audio", "auth", "auto-mode-setup", "auto-review", "auto-run", "autoapprove", "autocompact",
    "autofix-pr", "automate", "autopilot", "available", "award", "away", "awe", "background", "backpack", "bad",
    "badfeeling", "ban", "ban-ip", "banlist", "bark", "bashes", "bashful", "batch", "battleground", "bb", "beckon",
    "bedrock", "beep", "beg", "belch", "best-of-n", "bg", "bind", "bite", "blame", "blank", "bleed", "blink", "block",
    "blocked_by", "blocks", "blood", "blow", "blush", "board_move", "boggle", "bonk", "book", "bored", "bossbar",
    "bossdamage", "bounce", "bow", "branch", "brandish", "bravo", "brb", "bread", "breath", "brow", "browse", "btw",
    "bug", "bug-memory", "bullet", "burp", "busy", "button", "bye", "cackle", "caffeinate", "calendar", "call",
    "callout", "calm", "camera", "camerashake", "cancelaura", "cancelform", "cancelqueuedspell", "canvas", "cast",
    "castrandom", "castsequence", "cat", "catty", "cd", "challenge", "changeactionbar", "changelog", "changesetting",
    "channel", "charge", "charm", "chat", "chat-mode", "chatfilter", "chathelp", "chatinvite", "chatlist", "chatlog",
    "checkin_reminder", "checkpoint", "checkup", "cheer", "chew", "chicken", "chrome", "chronicle", "chuckle", "chug",
    "ckick", "clap", "claude-api", "claude-in-chrome", "clean", "clear", "clear_health_status", "clear_weight",
    "clearfocus", "clearspawnpoint", "cleartarget", "clearworldmarker", "click", "clikit", "clone", "close", "cloud",
    "cloud-environment", "cls", "code", "code-review", "cold", "collapse", "collapsible", "collect-debug-logs",
    "color", "combatlog", "comfort", "commands", "commend", "comment", "commercial", "commit", "compact",
    "completion", "composer", "compress", "compute", "concede", "confidential", "config", "confused", "congrats",
    "congratulate", "connect", "console", "context", "continue", "controlscheme", "convert_to_ticket", "copy",
    "copy-context", "copy-conversation-id", "copy-request-id", "copy_metadata", "corgi", "cost", "cough", "coverears",
    "cower", "crack", "create", "create-agent", "create-canvas", "create-hook", "create-instructions",
    "create-prompt", "create-rule", "create-skill", "create-subagent", "create_merge_request", "cringe", "crossarms",
    "cry", "csay", "ctcp", "cuddle", "curious", "cursor", "cursor-blame", "curtsey", "cwd", "cycle", "d", "daemon",
    "damage", "dance", "darkmode", "data", "datapack", "dataviz", "date", "daybreak", "daylock", "dcc", "deafen",
    "death", "debug", "debug-config", "decision", "dedicatedwsserver", "deep-planning", "deep-research",
    "defaultgamemode", "dehilight", "delegate", "delete", "delete-worktree", "deletepoll", "delfriend", "deop",
    "description", "design", "design-login", "design-sync", "desktop", "detach", "details", "devoice", "diagnose",
    "diagram", "dialog", "dialogue", "die", "diff", "difficulty", "ding", "dir", "directory", "disableaddons",
    "disableautoapprove", "disableyolo", "disagree", "disappointed", "disappointment", "disconnect", "dismisspet",
    "dismount", "div", "divider", "dm", "dnd", "doc", "docs", "doctor", "doh", "done", "doom", "doubt", "downgrade",
    "downloads", "draft", "drafts", "drink", "drool", "drop", "duck", "due", "duel", "dump", "dungeonfinder",
    "duplicate", "e", "eat", "echo", "edit", "editor", "editor-allowlist", "editor-model", "effect", "effort",
    "embarrass", "embed", "emoji", "emote", "emoteonly", "emoteonlyoff", "enableaddons", "enchant", "encourage",
    "endpoll", "enemy", "env", "epic", "equation", "equip", "equipset", "equipslot", "estimate", "estimate_time",
    "eval", "event", "eventtrace", "every", "excited", "exec", "execute", "exit", "expand", "experience",
    "experimental", "explain", "export", "export-gist", "export-session", "extension", "extensions", "eye", "eyebrow",
    "eyeroll", "facepalm", "faint", "farewell", "fart", "fast", "fear", "feast", "feedback", "fetchprofile",
    "fewer-permission-prompts", "ff", "ffa", "fidget", "file", "fill", "fillbiome", "find", "fist", "fix", "flap",
    "flee", "fleet", "flex", "flirt", "flop", "focus", "fog", "follow", "followers", "followersoff", "followme",
    "food", "footer", "forceload", "forfeit", "fork", "format", "framestack", "friend", "frown", "fullmute",
    "function", "g", "gamemode", "gamerule", "gametest", "gametips", "gasp", "gaze", "gemma", "gg", "gif", "gift",
    "giggle", "giphy", "git", "give", "glad", "glare", "gloat", "glower", "go", "goal", "going", "golfclap",
    "goodbye", "goto", "grats", "greet", "greetings", "grin", "groan", "group", "groupmsg", "grovel", "growl",
    "guffaw", "guild", "guilddemote", "guilddisband", "guildfinder", "guildinfo", "guildinvite", "guildleader",
    "guildmotd", "guildpromote", "guildquit", "guildremove", "guildroster", "h", "h1", "h2", "h3", "hail", "happy",
    "hash", "headache", "header", "heading", "healme", "health_status", "heapdump", "hello", "help", "helpme", "hi",
    "hiccup", "hide", "hidelivecaptions", "highfive", "hilight", "hiss", "holdhand", "holdit", "holler", "hooks",
    "hud", "huddle", "hug", "hungry", "hurry", "ide", "ide-context", "idea", "ig", "ignore", "ignores", "image",
    "immutableworld", "impatient", "import", "in-cloud", "inbox", "inc", "include", "incoming", "info", "init",
    "inputpermission", "insert", "insights", "inspect", "install-github-app", "install-slack-app", "instructions",
    "insult", "interactive", "internal_note", "introduce", "invite", "invite_people", "ios", "irssiproxy", "ison",
    "item", "iteration", "j", "jealous", "jfr", "jk", "join", "joke", "keep-alive", "keybindings", "keymap", "keys",
    "kick", "kickban", "kill", "kiss", "kneel", "knock", "knockout", "knuckles", "l", "label", "labels", "lastlog",
    "latex", "laugh", "lavish", "lay", "laydown", "layout", "leave", "leavechat", "leavevehicle", "lick", "lie",
    "liedown", "limits", "line-numbers", "linear", "link", "links", "lint", "list", "list-agents", "list-dirs",
    "listen", "load", "load-workspace", "local", "locate", "lock", "log", "login", "logout", "logs", "lol", "look",
    "loop", "loot", "lost", "love", "ls", "lsp", "luck", "lusers", "m", "macro", "macrohelp", "mad", "mainassist",
    "mainassistoff", "maintank", "maintankoff", "map", "map-refresh", "marker", "marketplace", "massage", "master",
    "masterybadge", "math", "max-mode", "mb", "mcp", "me", "meetnow", "memories", "memory", "mention", "mentions",
    "mercy", "merge", "merge-to-parent", "migrate-to-skills", "milestone", "mircdcc", "moan", "mobevent", "mobile",
    "mock", "mod", "mode", "model", "models", "moderator", "mods", "monitor", "moo", "moon", "motd", "mountspecial",
    "mourn", "move", "moveto", "msg", "multiline-mode", "music", "mute", "mutefull", "muteping", "mutepings",
    "muteself", "mutter", "n", "names", "nctcp", "nervous", "netsplit", "network", "new", "new-chat", "newchat",
    "newnotebook", "newrule", "newtask", "nick", "no", "noconcede", "nod", "noff", "noforfeit", "nopm", "noremake",
    "nosepick", "nosurrender", "note", "noterminate", "notice", "notifications", "notify", "np", "num", "object",
    "objection", "offer", "officer", "offline", "ok", "online", "oom", "op", "open", "openfire", "oper", "ops",
    "opus", "orchestrate", "orchestrator", "output-style", "owner", "p", "pack", "packstack", "page", "palm", "panel",
    "panic", "pardon", "pardon-ip", "part", "participants", "particle", "party", "passes", "password", "paste", "pat",
    "pause", "pdf", "peer", "peers", "peon", "people", "perf", "permission", "permissions", "personality", "pest",
    "pet", "petassist", "petattack", "petautocastoff", "petautocaston", "petautocasttoggle", "petdefensive",
    "petdismiss", "petfollow", "petmoveto", "petpassive", "pets", "petstay", "pick", "pin", "pinch", "ping",
    "pingmute", "pity", "pizza", "place", "plain", "plan", "playanimation", "played", "players", "playing",
    "playsound", "plead", "plot", "plugin", "plugin-authoring", "plugins", "point", "poke", "policies", "poll",
    "ponder", "posteffect", "pounce", "pout", "powerup", "pr", "pr-comments", "pr-fix-checks", "pr-merge", "pr-open",
    "pr-resolve-comments", "praise", "pray", "prediction", "privacy", "privacy-settings", "proactive", "project",
    "promise", "promote", "promote_to", "promote_to_incident", "prompts", "proud", "ps", "pt", "publish", "pulse",
    "punch", "purpose", "purr", "puzzled", "pvp", "pvpdeath", "pwd", "query", "question", "quit", "quote", "r",
    "radio", "raid", "raidfinder", "raidinfo", "raise", "random", "randomfavoritepet", "randompet", "rasp",
    "rate-limit-options", "raw", "rawlog", "rawr", "rc", "rdy", "react", "read", "read-only", "ready", "readycheck",
    "rear", "reasoning", "reasoning-effort", "reassign", "reassign_reviewer", "rebase", "recap", "recipe", "recode",
    "reconnect", "record", "red", "refactor", "refine", "refresh", "regret", "rehash", "relabel", "relate",
    "release-notes", "reload", "reload-plugins", "reload-skills", "reloadconfig", "reloadpacketlimitconfig", "remake",
    "remind", "reminder", "remote", "remote-control", "remote-env", "remove", "remove_child", "remove_contacts",
    "remove_due_date", "remove_email", "remove_estimate", "remove_iteration", "remove_label", "remove_milestone",
    "remove_parent", "remove_reviewer", "remove_time_estimate", "remove_time_spent", "remove_zoom", "removefriend",
    "rename", "reopen", "rep", "replaceitem", "reply", "report", "reportbug", "request_review", "requestinvite",
    "requests", "research", "reset", "reset-allowed-tools", "resetchat", "restart", "restart-session", "restore",
    "restrict", "resume", "retreat", "return", "revenge", "review", "review-bugbot", "review-security", "reviewer",
    "rewind", "ride", "rmreconns", "roar", "rofl", "roll", "rolleyes", "rotate", "routines", "rps", "rubber-duck",
    "rude", "ruffle", "rules", "run", "run-everything", "run-skill-generator", "run_pipeline", "rw", "s", "sad",
    "salute", "sandbox", "sandbox-add-read-dir", "sandbox-policy", "save", "save-all", "save-off", "save-on",
    "save-workspace", "saved", "saved-replies", "savelog", "say", "scared", "schedule", "schedulemessage", "scoff",
    "scold", "sconnect", "scoreboard", "scowl", "scratch", "script", "scriptevent", "scroll-speed", "scrollback",
    "sdk", "search", "security-review", "seed", "send", "sendshowstoreoffer", "server", "servlist", "session",
    "sessions", "set", "set_movement_authority", "set_parent", "setblock", "setidletimeout", "setmaxplayers",
    "settings", "settitle", "setup-bedrock", "setup-default-sandbox", "setup-github", "setup-terminal",
    "setup-vertex", "setuptests", "setworldspawn", "severity", "sexy", "sh", "shake", "shakefist", "share",
    "sharedchat", "shell", "shells", "shifty", "shimmy", "shindig", "shiver", "shoo", "shortcuts", "shout",
    "shoutout", "show-thinking", "showlivecaptions", "shrug", "shudder", "shush", "shy", "side", "sigh", "signal",
    "silence", "silly", "simplify", "sing", "sit", "skill-doctor", "skills", "slap", "sleep", "slides", "slow",
    "slowoff", "smack", "smell", "smile", "smirk", "smol", "snap", "snarl", "sneak", "sneeze", "snicker", "sniff",
    "snippet", "snort", "snub", "sob", "soothe", "sorry", "spar", "spawn", "spawnpoint", "spectate", "spend",
    "spend_time", "spit", "split-to-prs", "spoiler", "spoon", "spreadplayers", "squeal", "squelch", "squery", "squit",
    "stand", "stare", "startattack", "starttranscription", "stats", "status", "status-indicators", "statusbar",
    "statusline", "stickers", "stink", "stop", "stopattack", "stopcasting", "stoplivetranscription", "stopmacro",
    "stopsound", "stopspelltarget", "stopwatch", "strong", "structure", "strut", "subagents", "submit_review",
    "subscribe", "subscribers", "subscribersoff", "subtask", "summarize", "summarize_comments", "summon", "summonpet",
    "surprised", "surrender", "suspicious", "swapactionbar", "sweat", "swing", "sync", "t", "table", "tableflip",
    "tableinspect", "tag", "talk", "talkex", "talkq", "tap", "target", "target_branch", "targetenemy",
    "targetenemyplayer", "targetexact", "targetfriend", "targetfriendplayer", "targetlastenemy", "targetlastfriend",
    "targetlasttarget", "targetmarker", "targetparty", "targetraid", "tasks", "taunt", "team", "team-onboarding",
    "teammsg", "tease", "teleport", "tell", "tellraw", "template", "terminal", "terminal-setup", "terminate", "test",
    "testfor", "testforblock", "testforblocks", "tests", "text", "thank", "thanks", "theme", "think", "think-tokens",
    "thirsty", "threaten", "threshold", "tick", "tickingarea", "tickle", "time", "timeline", "timeout", "timetest",
    "tired", "title", "titleraw", "tm", "toc", "todo", "toggle", "togglechatfilter", "toggledownfall", "tokens",
    "tools", "topic", "tp", "trace", "trade", "train", "transfer", "trigger", "troubleshoot", "truce", "ts", "tts",
    "tui", "tuikit", "turnbullet", "twiddle", "ty", "type", "ultraplan", "ultrareview", "um", "unalias", "unapprove",
    "unassign", "unassign_reviewer", "unban", "unblock", "undo", "unignore", "uninvite", "uniquechat",
    "uniquechatoff", "unlabel", "unlink", "unload", "unlock", "unmod", "unmoderator", "unmonitor", "unmute",
    "unnotify", "unpause", "unpublish", "unquery", "unraid", "unrestrict", "unsilence", "unsubscribe", "unused",
    "unvip", "update", "update-cli-config", "update-config", "update-cursor-settings", "upgrade", "uptime", "usage",
    "usage-credits", "use", "user", "userandom", "userhost", "usetoy", "v", "ver", "verify", "version", "veto",
    "victory", "video", "vim", "violin", "vip", "vips", "visualize", "voice", "volunteer", "vote",
    "vulnerability_explain", "w", "wait", "wall", "wallops", "wargame", "warn", "warnings", "watch", "wave",
    "waypoint", "wb", "weak-model", "weather", "web", "web-setup", "weep", "weight", "welcome", "whine", "whisper",
    "whistle", "whitelist", "who", "whois", "whowas", "wicked", "wickedly", "window", "wink", "woot", "work",
    "workflow", "workflow-authoring", "workflows", "worktree", "worldborder", "worldbuilder", "worldmarker", "wp",
    "wrath", "wsserver", "xp", "yawn", "yay", "yell", "yes", "yolo", "zoom",
];

struct UserList {
    modified: Option<std::time::SystemTime>,
    words: Vec<String>,
}

static USER: Mutex<Option<UserList>> = Mutex::new(None);

/// 2026-10-14 (脆弱性の再調査 #1): 管理者権限・隔離されたアプリの中では、利用者が書き換えられる %APPDATA% のファイルを
/// 読まない・書かない (組み込みの一覧だけで判定し、学習しない)。置かれたリンクの先を管理者権限で書き換えてしまうため。
/// フォルダかファイルがリンク (シンボリックリンク・ジャンクション) なら使わない
fn user_file() -> Option<std::path::PathBuf> {
    static ALLOWED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ALLOWED.get_or_init(shared::winsec::private_data_allowed) {
        return None;
    }
    // 2026-11-10: 置き場は shared::app_data_dir (試験のビルドでは一時フォルダ)
    let dir = shared::app_data_dir()?;
    let linked = |p: &std::path::Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    let path = dir.join("slash_commands.txt");
    (!linked(&dir) && !linked(&path)).then_some(path)
}

/// 読むのは先頭の 64KB まで (巨大なファイルを置かれても全部は読まない)
fn read_user_file(path: &std::path::Path) -> Option<String> {
    use std::io::Read as _;
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(64 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

/// 利用者が足した語と学習した語。ファイルが変わったら読み直す
fn with_user_words<T>(f: impl FnOnce(&[String]) -> T) -> T {
    let path = user_file();
    let modified = path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
    let mut guard = USER.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().map_or(true, |u| u.modified != modified) {
        let words = path
            .and_then(|p| read_user_file(&p))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().trim_start_matches('/').to_ascii_lowercase())
            .filter(|l| !l.is_empty() && !l.starts_with('#') && l.len() <= 64)
            .collect();
        *guard = Some(UserList { modified, words });
    }
    f(&guard.as_ref().unwrap().words)
}

/// 知っているコマンドの頭か
pub fn is_command_prefix(prefix: &str) -> bool {
    let prefix = prefix.to_ascii_lowercase();
    BUILTIN.iter().any(|c| c.starts_with(&prefix)) || with_user_words(|words| words.iter().any(|c| c.starts_with(&prefix)))
}

/// / のあとに打った語 (小文字の英字だけ) を、英字のまま渡し続けるか
pub fn stays_latin(word: &str) -> bool {
    if is_command_prefix(word) {
        return true;
    }
    // ハイフンは のばす音 (ー) として読む (ra-menn = らーめん)。英単語の頭は ハイフンの前までで見る
    let letters = word.replace('-', "");
    if word.contains('-') {
        return letters.is_empty() || !auto_english::romaji_readable_prefix(&letters);
    }
    auto_english::english_word_prefix(word) || !auto_english::romaji_readable_prefix(word)
}

thread_local! {
    /// 日本語に戻した綴り (小文字)。次の確定か、英数モードで打ち終えた次の語まで持つ
    static LAST_SWITCH: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

pub fn note_switched(word: &str) {
    LAST_SWITCH.with_borrow_mut(|w| *w = Some(word.to_ascii_lowercase()));
}

/// 持っている綴りを捨てる (ほかの確定・フォーカスの移り)
pub fn forget_switched() {
    LAST_SWITCH.with_borrow_mut(|w| *w = None);
}

/// 覚える綴り: 利用者が英字に直したもの (`chosen`) が、日本語に戻した綴りで始まる英字のコマンド名なら、その小文字。`private` は覚えてはいけない欄
fn word_to_learn(switched: &str, chosen: &str, private: bool) -> Option<String> {
    let chosen = chosen.trim().to_ascii_lowercase();
    let command_like = !chosen.is_empty() && chosen.len() <= 64 && chosen.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b));
    (!private && !switched.is_empty() && command_like && chosen.starts_with(switched)).then_some(chosen)
}

/// 書きかけを確定した。`chose_latin` は利用者が英字を選んだか (F9・F10・候補の一覧の英字)。覚える綴りを返す。持っている綴りはどちらでも捨てる
pub fn latin_commit(committed: &str, chose_latin: bool) -> Option<String> {
    let switched = LAST_SWITCH.with_borrow_mut(|w| w.take())?;
    if !chose_latin {
        return None;
    }
    word_to_learn(&switched, committed, super::direct_field::private_field())
}

/// 英数モードで語を打ち終えた (latin_learn)。`private` はその語を打ち始めた欄が覚えてはいけない欄か。覚える綴りを返す。
/// 空の語では持っている綴りを捨てない (/ などの記号だけで区切られたとき)
pub fn latin_word_typed(word: &str, private: bool) -> Option<String> {
    if word.is_empty() {
        return None;
    }
    let switched = LAST_SWITCH.with_borrow_mut(|w| w.take())?;
    word_to_learn(&switched, word, private || super::direct_field::private_field())
}

/// 英字のつもりだった綴りを slash_commands.txt に足す
/// 覚えてはいけないとき (管理者・隔離されたアプリ、覚えてはいけない欄。composition::may_learn) は書かない
pub fn learn(word: &str) {
    if cfg!(test) || !super::composition::may_learn() {
        return;
    }
    let word = word.trim().to_ascii_lowercase();
    if word.is_empty() || word.len() > 64 || !word.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b)) {
        return;
    }
    if is_command_prefix(&word) && with_user_words(|w| w.iter().any(|c| *c == word)) {
        return;
    }
    let Some(path) = user_file() else { return };
    let mut text = read_user_file(&path).unwrap_or_else(|| {
        "# 行頭の / のあとで英字のまま渡すコマンド名 (1 行 1 つ、/ は付けなくてよい)。azooKey が学習したものも足される\n".to_string()
    });
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&word);
    text.push('\n');
    // 別名に書いてから置き換える (書いている途中で止まっても元のファイルが壊れない)
    let temporary = path.with_extension(format!("txt.{}.tmp", std::process::id()));
    if std::fs::write(&temporary, text).is_err() || std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return;
    }
    crate::debug_log::write(&format!("slash command learned: {word}"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_user_file_is_read_only_up_to_64kb() {
        let path = std::env::temp_dir().join(format!("azk_slash_cap_{}.txt", std::process::id()));
        std::fs::write(&path, "a".repeat(200 * 1024)).unwrap();
        let text = super::read_user_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(text.len(), 64 * 1024);
    }

    use super::*;

    /// 打った綴りのどこで日本語に戻るか (戻らなければ None)。AZK_SLASH_CHECK=1 で実際の単語表で確かめる
    fn switch_at(word: &str) -> Option<usize> {
        (1..=word.len()).find(|&k| !stays_latin(&word[..k]))
    }

    #[test]
    fn slash_words_with_real_dictionaries() {
        if std::env::var_os("AZK_SLASH_CHECK").is_none() { return; }
        let mut report = String::new();
        for w in ["model", "compact", "clear", "review", "usage", "resume", "init", "mnt", "usr", "home", "help", "codex", "skills",
            "wouttyautozettaininihonngo", "woutsu", "korehadou", "ashitanoyotei", "usagi", "kyouhaiitenki", "nihongo", "sakeno", "tesuto",
            "matomete", "hiraganade", "eigodekaku", "ra-menn", "ko-hi-", "su-pa-de", "pr-comments", "add-dir", "release-notes", "terminal-setup", "eigo-chobun", "gamemode", "kill", "tp", "give", "remind", "giphy", "todo", "callout", "assign", "label", "explain", "fix", "tests", "kokodemiteta", "kinounoyotei", "tsugihadou", "mouikkai"] {
            report += &format!("{w}\t{:?}\n", switch_at(w).map(|k| &w[..k]));
        }
        if let Ok(out) = std::env::var("AZK_SLASH_OUT") { std::fs::write(out, &report).unwrap(); }
    }

    /// 日本語に戻した書きかけを消しただけでは覚えない。消したあとの確定は、英字を選んだものだけ
    #[test]
    fn deleting_the_switched_composition_does_not_learn() {
        note_switched("kyouha");
        // 書きかけを消した (Escape・Backspace) あとに、日本語を打って確定した
        assert_eq!(latin_commit("今日は", false), None);
        assert_eq!(latin_commit("kyouha", true), None, "確定で持っている綴りを捨てる");
        note_switched("kyouha");
        assert_eq!(latin_word_typed("", false), None, "記号だけの区切りでは捨てない");
        assert_eq!(latin_word_typed("ashita", false), None, "違う語");
        assert_eq!(latin_word_typed("kyouha", false), None, "英数モードの次の語で捨てた");
        assert_eq!(latin_commit("kyouha", true), None);
    }

    /// 利用者が英字に直して確定したら覚える (F9・F10・候補の英字、または消して英数モードで同じ綴りを打った)
    #[test]
    fn a_latin_commit_of_the_switched_spelling_learns() {
        note_switched("kyouha");
        assert_eq!(latin_commit("kyouha", true), Some("kyouha".to_string()));
        note_switched("Kyouha");
        assert_eq!(latin_commit("KYOUHA", true), Some("kyouha".to_string()), "F10 の大文字の形も小文字で覚える");
        note_switched("kyouha");
        assert_eq!(latin_commit("kyouharu", true), Some("kyouharu".to_string()), "続けて打った綴りごと");
        note_switched("kyouha");
        assert_eq!(latin_commit("kyou", true), None, "戻した綴りで始まらない");
        note_switched("kyouha");
        assert_eq!(latin_commit("ｋｙｏｕｈａ", true), None, "全角はコマンド名でない");
        assert_eq!(latin_commit("kyouha", true), None, "日本語に戻していない");
        note_switched("kyouha");
        assert_eq!(latin_word_typed("kyouha", false), Some("kyouha".to_string()));
    }

    /// 覚えてはいけない欄 (パスワード・PIN・IS_PRIVATE など、direct_field::private_field) では、どちらの道でも覚えない
    #[test]
    fn private_fields_never_learn() {
        use windows::Win32::UI::TextServices::{IS_PASSWORD, IS_PRIVATE};
        for scope in [IS_PRIVATE.0, IS_PASSWORD.0] {
            super::super::direct_field::note_scopes(Some(&[scope]));
            note_switched("kyouha");
            assert_eq!(latin_commit("kyouha", true), None, "{scope}");
            note_switched("kyouha");
            assert_eq!(latin_word_typed("kyouha", false), None, "{scope}");
        }
        super::super::direct_field::note_scopes(None);
        note_switched("kyouha");
        assert_eq!(latin_word_typed("kyouha", true), None, "語を打ち始めた欄が覚えてはいけない欄");
        note_switched("kyouha");
        assert_eq!(latin_commit("kyouha", true), Some("kyouha".to_string()), "印を外せば覚える");
    }

    #[test]
    fn builtin_commands_are_lowercase_ascii() {
        for c in BUILTIN {
            assert!(!c.is_empty() && c.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_:".contains(&b)), "{c}");
        }
    }
}
