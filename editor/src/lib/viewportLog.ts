// A line from the viewport's script log is a plain string; this decides how
// the Console shows it - its level, and the script and line it is about, so
// the row gets its colour and its `file.lua:21` link.

export interface ViewportLogLine {
  level: "info" | "warn" | "error";
  /** The message as the Console shows it: the tags that say where it came
   *  from are taken off, since the row's source column shows that. */
  text: string;
  scriptPath?: string;
  scriptLine?: number;
  /** Where a line not about a script came from. */
  source?: string;
}

/** What a script failing to compile or run says, in Lua's and the host's words. */
const ERROR_WORDS =
  /^\[ERROR\]|^Failed to load script|^Error|\bfailed:|syntax error|attempt to (call|index|perform|compare|concatenate)|stack traceback|\bunexpected symbol\b|'end' expected|\bpanicked\b/i;

/** A summary that counted failures ("errors=1") is a warning: the error itself was its own line. */
const WARN_WORDS = /^\[Warning\]|^\[WARN\]|\bwarning\b|\berrors=[1-9]/i;

/** `scripts/player.lua:21`, or a script named without a line. */
const PATH_LINE = /([\w./\\-]+\.(?:lua|js)):(\d+)/;
const PATH_BRACKET = /\[([^\]]+\.(?:lua|js))\]/;
const PATH_QUOTED = /'([^']+\.(?:lua|js))'/;

/** `[@scripts/player.lua:18] ` - where a `print` or `log` was called
 *  (lua-runtime.ts puts it there). */
const CALLER = /\[@([^\]\s]+):(\d+)\]\s?/;

/** `[viewport] ` and friends: who said it, when no script did. */
const SOURCE_TAG = /^\[(viewport|scene|package|packages|editor|engine|runtime)\]\s*/i;

export function classifyViewportLog(msg: string): ViewportLogLine {
  const caller = msg.match(CALLER);
  let text = caller ? msg.replace(CALLER, "") : msg;
  const tag = text.match(SOURCE_TAG);
  if (tag) text = text.slice(tag[0].length);

  const level = ERROR_WORDS.test(text) ? "error" : WARN_WORDS.test(text) ? "warn" : "info";
  const path = (p: string) => p.replace(/\\/g, "/");
  if (caller) {
    return { level, text, scriptPath: path(caller[1]), scriptLine: Number(caller[2]) };
  }
  const withLine = text.match(PATH_LINE);
  if (withLine) {
    return { level, text, scriptPath: path(withLine[1]), scriptLine: Number(withLine[2]) };
  }
  const named = text.match(PATH_BRACKET) ?? text.match(PATH_QUOTED);
  if (named) return { level, text, scriptPath: path(named[1]) };
  return { level, text, source: tag ? tag[1].toLowerCase() : "script" };
}
