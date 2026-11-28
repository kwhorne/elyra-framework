// Every word @elyra/ui shows, in English (RFC 0056).
//
// Each is also `elyra.<component>.<word>` in `framework/lang/en.json` (a test
// keeps the two the same), and the app's catalog translates it: its own
// message for the key, else the framework's Norwegian, else this English. A
// component's `labels` prop wins over all of them, for that one instance.
//
// A word with parameters is `[message, ...names]`: the component calls it
// with the values in that order (`words.noMatch(term)`). Or `[message, fn]`,
// where `fn` turns the call's arguments into the parameters. `:count` picks
// the plural segment, as `$tc` does.

import { fromStore } from "svelte/store";
import { frameworkWords, phrase } from "@elyra/runtime";

const people = (names) => ({ count: names.length, first: names[0] ?? "", second: names[1] ?? "" });

export const WORDS = {
  accounts: {
    signOut: ["Sign out of :host", "host"],
    signOutAll: "Sign out of all",
    add: "Add an account…",
    pending: [
      "{0} Changes made on :host haven't been sent yet. Sign out and discard them?|{1} A change made on :host hasn't been sent yet. Sign out and discard it?|[2,*] :count changes made on :host haven't been sent yet. Sign out and discard them?",
      "host",
      "count",
    ],
    pendingAll: "Some changes haven't been sent yet. Sign out of every server and discard them?",
    discard: "Sign out and discard",
    waiting: [":count waiting", "count"],
    refused: [":count refused", "count"],
    attention: "Changes refused on another server",
  },
  avatargroup: {
    label: "People",
    more: [":count more: :names", "count", "names"],
  },
  breadcrumbs: {
    label: "Breadcrumb",
  },
  callout: {
    dismiss: "Dismiss",
  },
  combobox: {
    placeholder: "Search…",
    empty: "No matches",
    searching: "Searching…",
    busy: "Searching",
    clear: "Clear",
  },
  datepicker: {
    choose: "Choose date",
    chooseWith: ["Choose date, :date", "date"],
    clear: "Clear",
    previous: "Previous month",
    next: "Next month",
    today: "Today",
  },
  daterange: {
    dates: "Dates",
    start: "Start date",
    end: "End date",
    choose: "Choose dates",
    chooseWith: ["Choose dates, :range", "range"],
    from: ["From :date", "date"],
    until: ["Until :date", "date"],
    between: [":start to :end", "start", "end"],
    clear: "Clear",
    presets: "Ranges",
    previous: "Previous month",
    next: "Next month",
    pickFirst: "Pick the first day",
    pickLast: "Pick the last day",
    today: "Today",
    last7: "Last 7 days",
    last30: "Last 30 days",
    thisMonth: "This month",
    lastMonth: "Last month",
    thisYear: "This year",
  },
  editing: {
    title: "Editing",
    open: "Open",
    saving: "Saving",
    saved: "Saved",
    waiting: "Waiting",
    conflict: "Changed on the server",
    denied: "Not allowed",
    failed: "Refused",
    showInFinder: "Show in Finder",
    showInFolder: "Show in folder",
    unsaved: [
      "The last save of :name hasn't reached the server. Stop editing it anyway? That save is lost.",
      "name",
    ],
    stop: "Stop editing",
    keepMine: "Keep mine",
    takeTheirs: "Take theirs",
    show: "Open",
    done: "Done",
    emptyTitle: "Nothing open",
    emptyDescription: "No file is open in another app.",
    needsYou: "Needs you",
    openHeading: "Open",
    savedAt: ["Last saved to the server :when", "when"],
  },
  empty: {
    title: "Nothing here yet",
  },
  filefield: {
    choose: "Choose a file…",
    replace: "Replace…",
    remove: "Remove",
    keep: "Keep the current one",
    none: "No file",
    drop: "or drop one on the window",
  },
  microphone: {
    button: "Dictate",
    recording: "Recording",
    transcribing: "Transcribing…",
    hint: "Click, or hold, to talk",
  },
  modal: {
    close: "Close",
  },
  notifications: {
    unread: [":count unread", "count"],
    markAll: "Mark all read",
    more: "More",
    all: "See all",
    empty: "Nothing yet",
    offline: "Offline: this is the list as it was. Reading one is sent once you're back.",
    untitled: "Notification",
    justNow: "just now",
  },
  outbox: {
    title: "Changes",
    write: [":verb :path", "verb", "path"],
    create: "Create",
    update: "Update",
    delete: "Delete",
    conflict: "Changed meanwhile",
    validation: "Not valid",
    denied: "Not allowed",
    gone: "Gone",
    refused: "Refused",
    emptyTitle: "Nothing waiting",
    emptyDescription: "Every change has reached the server.",
    refusedHeading: "Refused",
    waitingHeading: "Waiting",
    keepMine: "Keep mine",
    takeTheirs: "Take theirs",
    sendAgain: "Send again",
    discard: "Discard",
    discardConfirm: "Discard this change? It won't be sent.",
    saved: ["Saved :when", "when"],
    tried: ["{1} tried once|[2,*] tried :count times", "count"],
  },
  pagination: {
    label: "Pagination",
    previous: "Previous",
    next: "Next",
    page: ["Page :page", "page"],
  },
  presence: {
    label: "Also here",
    editing: ["{1} :first is editing|{2} :first and :second are editing|[3,*] :count people are editing", people],
    note: "editing",
    follow: "Follow…",
    following: ["Following :name", "name"],
    away: [":name is somewhere you can't follow", "name"],
    asking: ["Asking :name…", "name"],
    followedBy: ["{1} :first is following you|[2,*] :count people are following you", people],
    stop: "Stop",
  },
  select: {
    placeholder: "Choose…",
    search: "Search…",
    searchLabel: "Search the options",
    empty: "No matches",
  },
  report: {
    page: ["Page :page of :pages", "page", "pages"],
  },
  shell: {
    main: "Main",
  },
  sidebar: {
    label: "Main",
  },
  sparkline: {
    empty: "No data",
    summary: ["From :first to :last, between :min and :max", "first", "last", "min", "max"],
  },
  spinner: {
    label: "Loading",
  },
  splitpane: {
    label: "Resize",
  },
  statusbar: {
    label: "Status",
    offline: "Offline",
    offlineSince: ["Offline since :time", "time"],
    offlineSimulated: "Offline (simulated)",
    uploading: "Uploading",
    downloading: "Downloading",
    uploadingName: ["Uploading :name", "name"],
    downloadingName: ["Downloading :name", "name"],
    pending: ["{1} 1 change waiting|[2,*] :count changes waiting", "count"],
    refused: ["{1} 1 change refused|[2,*] :count changes refused", "count"],
    needYou: ["{1} 1 file needs you|[2,*] :count files need you", "count"],
    saving: "Saving",
    savingName: ["Saving :name", "name"],
    waitingSaves: ["{1} 1 save waiting|[2,*] :count saves waiting", "count"],
    editingOne: ["Editing :name", "name"],
    editingMany: ["Editing :count files", "count"],
    assistant: "Assistant",
  },
  table: {
    search: "Search…",
    failed: "Couldn't load this list",
    empty: "Nothing here yet",
    noMatch: ["Nothing matches “:term”", "term"],
    total: [":count in all", "count"],
    previous: "Previous",
    next: "Next",
    page: ["Page :page", "page"],
    selected: [":count selected", "count"],
    clear: "Clear",
    searchLabel: "Search",
    selectAll: "Select all on this page",
    selectRow: ["Select :row", (row, name) => ({ row: name })],
    resize: ["Resize :column", "column"],
    pending: "Not sent yet",
    actions: "Actions",
    // Taking it out (RFC 0066).
    copy: ["{1} Copy row|[2,*] Copy :count rows", "count"],
    copyTable: "Copy table",
    saveCsv: "Save as CSV…",
    copied: "Copied",
    exportFailed: ["Couldn't copy or save the table: :message", "message"],
    // Totals (RFC 0067).
    totalRow: "Total",
    pageTotal: "This page",
    selectedTotal: ["Selected (:count)", "count"],
  },
  treeview: {
    loading: "Loading…",
    failed: "Couldn't load this.",
    retry: "Try again",
  },
};

/** The English message of a word: `"Clear"`, or the first of `[message, ...]`. */
export const englishOf = (entry) => (Array.isArray(entry) ? entry[0] : entry);

/** The parameters a call of a word gives its message. */
function paramsOf(entry, args) {
  const [, ...how] = entry;
  if (typeof how[0] === "function") return how[0](...args);
  return Object.fromEntries(how.map((name, i) => [name, args[i] ?? ""]));
}

/**
 * The words of one component: `words.clear` is a string, a word with
 * parameters a function (`words.noMatch(term)`). Each reads `labels()` first,
 * then the app's catalog, then the English, and follows the locale.
 *
 * Call it while the component is set up. `table` is another package's
 * words in the same form (`@elyra/charts`'), `elyra.<component>.<word>` too.
 */
export function useWords(component, labels = () => ({}), table = WORDS) {
  const english = table[component];
  if (!english) throw new Error(`@elyra/ui: no words for \`${component}\``);
  const catalog = fromStore(frameworkWords);
  const words = {};
  for (const [name, entry] of Object.entries(english)) {
    const say = (args) => {
      const own = labels()?.[name];
      if (own !== undefined) return typeof own === "function" ? own(...args) : own;
      const { locale, message } = catalog.current;
      const found = message(`elyra.${component}.${name}`);
      const params = Array.isArray(entry) ? paramsOf(entry, args) : {};
      // A count that isn't one (none known yet) takes the `{0}` segment.
      if ("count" in params && typeof params.count !== "number") params.count = Number(params.count) || 0;
      return phrase(found ?? englishOf(entry), params, found === undefined ? "en" : locale);
    };
    Object.defineProperty(words, name, {
      enumerable: true,
      get: () => (Array.isArray(entry) ? (...args) => say(args) : say([])),
    });
  }
  return words;
}

/** The app's locale once its catalog has loaded, else `undefined` (the webview's). */
export function useAppLocale() {
  const catalog = fromStore(frameworkWords);
  return () => catalog.current.locale || undefined;
}
