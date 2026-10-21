const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const BLANK = "data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==";
// Version de pochette par fichier : change quand on remplace une pochette, pour contourner le cache.
const coverVer = new Map();
const coverUrl = (path) => "http://cover.localhost/" + encodeURIComponent(path) + (coverVer.has(path) ? "?v=" + coverVer.get(path) : "");
const icon = (name, cls = "") => `<svg class="ic ${cls}" aria-hidden="true"><use href="#i-${name}"/></svg>`;
const PLAY_SVG = icon("play", "solid");
const EQBARS = `<span class="eqbars" aria-label="En lecture"><i></i><i></i><i></i></span>`;

// Fait défiler un conteneur jusqu'à un de ses enfants, sans toucher aux autres zones.
function scrollToChild(container, child, align = "start", smooth = false) {
  if (!container || !child) return;
  const c = container.getBoundingClientRect();
  const r = child.getBoundingClientRect();
  let top = container.scrollTop + (r.top - c.top);
  if (align === "center") top -= (c.height - r.height) / 2;
  if (align === "nearest") {
    if (r.top >= c.top && r.bottom <= c.bottom) return;
    if (r.bottom > c.bottom) top -= c.height - r.height;
  }
  container.scrollTo({ top: Math.max(0, top), behavior: smooth ? "smooth" : "auto" });
}

// --- données ---------------------------------------------------------------------

let tracks = [];
let albums = [];          // { key, title, artist, tracks, duration, formats, year, genre }
let artists = [];         // { name, albums, trackCount }
let albumByKey = new Map();
let artistByName = new Map();
let trackByPath = new Map();
let root = null;          // dossier principal de la bibliothèque
let extraRoots = [];      // dossiers supplémentaires
const allRoots = () => [root, ...extraRoots].filter(Boolean);
const rootOf = (p) => allRoots().find((r) => p === r || p.startsWith(r.replace(/[\\/]+$/, "") + sep(r))) || root;
// Préférences d'affichage (enregistrées dans les réglages, gérées ici).
let ui = {
  ignoreArticles: true, compilations: "various", zoom: 100, density: "normal", startView: "music", badges: true,
  eqPresets: {}, eqPerDevice: false, eqByDevice: {}, ambientStyle: "cover", ambientClock: true,
};
function saveUi() {
  invoke("set_ui", { ui });
}
let state = null;         // dernier état du moteur
let queue = [];           // file de lecture envoyée au moteur

const collator = new Intl.Collator("fr", { sensitivity: "base", numeric: true });
// Les noms qui ne diffèrent que par la casse, les accents ou la ponctuation (« KATY PERRY » / « Katy Perry »)
// désignent le même artiste ou le même album : on les regroupe sous une seule orthographe.
// Clé de regroupement : sans casse, sans accents, sans espaces ni ponctuation
// (« Feu ! Chatterton » = « Feu! Chatterton », « Frédéric » = « Frederic »).
const nameKey = (s) =>
  s.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase("fr").replace(/[^\p{L}\p{N}]/gu, "") || s.trim().toLowerCase();
let canonArtist = new Map(); // clé → orthographe retenue
const rawArtistOf = (t) => (t.albumArtist || t.artist || "Artiste inconnu").trim();
const artistOf = (t) => {
  if (compilation.has(t.path)) return VARIOUS;
  const raw = rawArtistOf(t);
  return canonArtist.get(nameKey(raw)) || raw;
};
const albumKeyOf = (t) => artistOf(t) + "\u0001" + nameKey(t.album || "Album inconnu");
// Tri : « The Cure » rangé à C, « Les Négresses Vertes » à N (si l'option est active).
const sortName = (n) => (ui.ignoreArticles ? n.replace(/^(the|les|la|le|los|las|el|die|der|das)\s+|^l['’]\s*/i, "") : n);
const VARIOUS = "Artistes divers";
const VARIOUS_RE = /^(various( artists)?|va|v\.a\.|artistes? divers|divers|compilation|multi[- ]?artistes?)$/i;
let compilation = new Set(); // chemins des titres de compilations

// Orthographe retenue : la plus fréquente, et à égalité celle qui n'est pas tout en majuscules.
function pickSpelling(counts) {
  const isCaps = (n) => n === n.toUpperCase() && n !== n.toLowerCase();
  const accents = (n) => (n.normalize("NFD").match(/\p{M}/gu) || []).length;
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || isCaps(a[0]) - isCaps(b[0]) || accents(b[0]) - accents(a[0]) || a[0].localeCompare(b[0]))[0][0];
}

/** Compilations : « Various Artists » en artiste d'album, ou un même album (même dossier)
 *  partagé par au moins 3 artistes différents sans artiste d'album commun. */
function findCompilations() {
  compilation = new Set();
  if (ui.compilations !== "various") return;
  const groups = new Map();
  for (const t of tracks) {
    if (VARIOUS_RE.test((t.albumArtist || "").trim())) {
      compilation.add(t.path);
      continue;
    }
    const dir = t.path.slice(0, Math.max(t.path.lastIndexOf("\\"), t.path.lastIndexOf("/")));
    const k = dir + "\u0001" + nameKey(t.album || "");
    let g = groups.get(k);
    if (!g) groups.set(k, (g = { paths: [], albumArtists: new Set() }));
    g.paths.push(t.path);
    g.albumArtists.add(nameKey(t.albumArtist || t.artist || ""));
  }
  for (const g of groups.values()) if (g.albumArtists.size >= 3) g.paths.forEach((p) => compilation.add(p));
}

function index() {
  findCompilations();
  const variants = new Map();
  for (const t of tracks) {
    const raw = rawArtistOf(t);
    const k = nameKey(raw);
    if (!variants.has(k)) variants.set(k, new Map());
    const m = variants.get(k);
    m.set(raw, (m.get(raw) || 0) + 1);
  }
  canonArtist = new Map([...variants].map(([k, m]) => [k, pickSpelling(m)]));
  trackByPath = new Map(tracks.map((t) => [t.path, t]));
  albumByKey = new Map();
  for (const t of tracks) {
    const key = albumKeyOf(t);
    let a = albumByKey.get(key);
    if (!a) {
      a = { key, title: t.album || "Album inconnu", artist: artistOf(t), tracks: [], duration: 0, size: 0, formats: new Set(), year: null, genre: "" };
      albumByKey.set(key, a);
    }
    a.tracks.push(t);
    a.duration += t.duration || 0;
    a.size += t.size || 0;
    a.formats.add(fmtLabel(t));
    if (t.year && (!a.year || t.year < a.year)) a.year = t.year;
    if (t.genre && !a.genre) a.genre = t.genre;
  }
  for (const a of albumByKey.values()) a.tracks = sortAlbumTracks(a.tracks);
  albums = [...albumByKey.values()].sort((x, y) => collator.compare(x.artist, y.artist) || collator.compare(x.title, y.title));

  artistByName = new Map();
  for (const a of albums) {
    let ar = artistByName.get(a.artist);
    if (!ar) artistByName.set(a.artist, (ar = { name: a.artist, albums: [], trackCount: 0 }));
    ar.albums.push(a);
    ar.trackCount += a.tracks.length;
  }
  // Discographie : du plus récent au plus ancien, comme MusicBee.
  for (const ar of artistByName.values()) ar.albums.sort((x, y) => (y.year || 0) - (x.year || 0) || collator.compare(x.title, y.title));
  artists = [...artistByName.values()].sort((x, y) => collator.compare(sortName(x.name), sortName(y.name)));
}

// --- cœurs et playlists ----------------------------------------------------------------

let coll = { version: 0, likedTracks: [], likedAlbums: [], playlists: [] };
let likedT = new Set();
let likedA = new Set();
// Clé d'un album aimé, identique sur le téléphone : « artiste|album » sans casse, accents ni ponctuation.
const likeKeyOf = (a) => nameKey(a.artist) + "|" + nameKey(a.title);
const heartBtn = (path, cls = "heart") =>
  `<button class="${cls} ${likedT.has(path) ? "on" : ""}" data-like-track="${esc(path)}" aria-label="J'aime" title="J'aime">${icon("heart")}</button>`;
const albumHeart = (a) =>
  `<button class="heart heart-album ${likedA.has(likeKeyOf(a)) ? "on" : ""}" data-like-album="${esc(a.key)}" aria-label="J'aime l'album" title="J'aime l'album">${icon("heart")}</button>`;

function setColl(c) {
  if (!c || c.version < coll.version) return;
  coll = c;
  likedT = new Set(c.likedTracks);
  likedA = new Set(c.likedAlbums);
  paintHearts();
  if (route.view === "playlists" && !document.querySelector(".pl-rename")) render();
}
function paintHearts() {
  document.querySelectorAll("[data-like-track]").forEach((b) => b.classList.toggle("on", likedT.has(b.dataset.likeTrack)));
  document.querySelectorAll("[data-like-album]").forEach((b) => {
    const a = albumByKey.get(b.dataset.likeAlbum);
    b.classList.toggle("on", !!a && likedA.has(likeKeyOf(a)));
  });
  const al = $("amb-like");
  if (al && al.dataset.likeTrack) al.classList.toggle("on", likedT.has(al.dataset.likeTrack));
  const np = $("np-like");
  if (np) {
    np.hidden = !state?.path;
    np.dataset.likeTrack = state?.path || "";
    np.classList.toggle("on", likedT.has(state?.path));
  }
}
async function collOp(op) {
  try {
    const r = await invoke("collection_op", { op });
    setColl(r.collections);
    return r.id ?? true;
  } catch (e) {
    toast(String(e), "error");
    return null;
  }
}
async function likeTracks(paths, on) {
  for (const path of paths) await collOp({ op: "likeTrack", path, on });
}
async function newPlaylist(paths = [], open = false) {
  const n = coll.playlists.length + 1;
  const r = await collOp({ op: "create", name: `Nouvelle playlist ${n}`, paths });
  const id = typeof r === "string" ? r : null;
  if (id && open) go({ view: "playlists", pl: id, rename: true });
  return id;
}
/** Menu « Ajouter à une playlist » : les playlists existantes, ou une nouvelle. */
function playlistPicker(x, y, paths) {
  const add = async (p) => {
    if (await collOp({ op: "add", id: p.id, paths })) toast(`${plural(paths.length, "titre ajouté", "titres ajoutés")} à « ${p.name} »`);
  };
  showMenu(x, y, [
    {
      label: "Nouvelle playlist",
      icon: "plus",
      run: async () => {
        const id = await newPlaylist(paths, true);
        if (id) toast(`Playlist créée avec ${plural(paths.length, "titre", "titres")}`);
      },
    },
    ...(coll.playlists.length ? ["-"] : []),
    ...coll.playlists.map((p) => ({ label: p.name, icon: "list-music", run: () => add(p) })),
  ]);
}
listen("collections", (e) => setColl(e.payload));

// --- utilitaires ------------------------------------------------------------------

const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const nf = (n) => n.toLocaleString(LOCALE);
const DEC = LANG === "en" ? "." : ",";
const fmtTime = (s) => {
  s = Math.max(0, Math.floor(s || 0));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${r}` : `${m}:${r}`;
};
const fmtLong = (s) => {
  const m = Math.round((s || 0) / 60);
  return m >= 60 ? `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")}` : `${m} min`;
};
const fmtSize = (b) => (b >= 1e9 ? (b / 1e9).toFixed(1) + (LANG === "en" ? " GB" : " Go") : (b / 1e6).toFixed(1) + (LANG === "en" ? " MB" : " Mo")).replace(".", DEC);
const khz = (f) => (f % 1000 === 0 ? String(f / 1000) : (f / 1000).toFixed(1).replace(".", DEC));
function fmtLabel(t) {
  if (t.bitDepth && t.sampleRate) return `${t.format} ${t.bitDepth}/${khz(t.sampleRate)}`;
  return t.format;
}
const channels = (n) => (n === 1 ? "Mono" : n === 2 ? "Stéréo" : n ? `${n} canaux` : "");
const plural = (n, one, many) => `${nf(n)} ${tr(isPlural(n) ? many : one)}`;
const sep = (p) => (p.includes("\\") ? "\\" : "/");
const img = (path, cls = "cover") =>
  `<img class="${cls}" loading="lazy" decoding="async" alt="" src="${esc(coverUrl(path))}" onload="this.classList.add('ld')" onerror="this.onerror=null;this.src='${BLANK}'">`;
// Photo d'artiste : on affiche tout de suite la pochette d'un album, puis on la remplace
// par la photo de l'artiste si elle existe (artist.jpg du dossier ou photo en ligne).
const artistVer = new Map();
const artistPhotoUrl = (ar) =>
  "http://artist.localhost/" + encodeURIComponent(ar.name) + "?t=" + encodeURIComponent(ar.albums[0].tracks[0].path) + (artistVer.has(ar.name) ? "&v=" + artistVer.get(ar.name) : "");
// Vignette d'artiste : pochette du premier album qui en a une, en essayant les albums un par un.
window.nextCover = (el) => {
  const rest = (el.dataset.alt || "").split("\n").filter(Boolean);
  if (!rest.length) {
    el.onerror = null;
    el.src = BLANK;
    return;
  }
  el.dataset.alt = rest.slice(1).join("\n");
  el.src = rest[0];
};
function artistImg(ar, cls = "cover") {
  const [first, ...rest] = ar.albums.slice(0, 8).map((a) => coverUrl(a.tracks[0].path));
  return `<img class="${cls}" loading="lazy" decoding="async" alt="" src="${esc(first)}" data-alt="${esc(rest.join("\n"))}" data-artist-src="${esc(artistPhotoUrl(ar))}" onload="this.classList.add('ld')" onerror="nextCover(this)">`;
}
// Ne charge les photos que des vignettes visibles, pour ne pas lancer 200 recherches d'un coup.
const photoObserver = new IntersectionObserver(
  (entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      const el = e.target;
      photoObserver.unobserve(el);
      const url = el.dataset.artistSrc;
      const probe = new Image();
      probe.onload = () => {
        if (el.dataset.artistSrc === url) el.src = url;
      };
      probe.src = url;
    }
  },
  { rootMargin: "200px" }
);
function hydrateArtistPhotos() {
  document.querySelectorAll("img[data-artist-src]:not([data-obs])").forEach((el) => {
    el.dataset.obs = "1";
    photoObserver.observe(el);
  });
}
const sortAlbumTracks = (list) =>
  [...list].sort((a, b) => (a.disc || 1) - (b.disc || 1) || (a.track || 0) - (b.track || 0) || collator.compare(a.path, b.path));
const firstLetter = (name) => {
  const c = name.normalize("NFD").replace(/[̀-ͯ]/g, "").charAt(0).toUpperCase();
  return c >= "A" && c <= "Z" ? c : "#";
};

// --- lecture -------------------------------------------------------------------------

function playList(list, i = 0) {
  if (!list.length) return;
  // Aléatoire : le titre choisi d'abord, puis le reste mélangé.
  if (shuffleOn && list.length > 1) {
    const first = list[i] || list[0];
    list = [first, ...smartShuffle(list.filter((t, k) => k !== i))];
    i = 0;
  }
  queue = list;
  invoke("play", { paths: list.map((t) => t.path), index: i });
}

/** Mélange qui évite d'enchaîner deux titres du même artiste quand c'est possible. */
function smartShuffle(list) {
  const a = [...list];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  const art = (t) => nameKey(artistOf(t));
  for (let i = 1; i < a.length; i++) {
    if (art(a[i]) !== art(a[i - 1])) continue;
    const k = a.findIndex((t, j) => j > i && art(t) !== art(a[i - 1]));
    if (k > i) [a[i], a[k]] = [a[k], a[i]];
  }
  return a;
}

// --- modes de lecture : aléatoire, fin de file (arrêt, répéter, radio), reprise ---------------

let shuffleOn = false;
let resumeOn = true;
let audio = {
  replaygain: "off", rgPreamp: 0, rgClip: true, crossfade: 0, crossfadeAlbums: false, fadeMs: 150,
  maxVolume: 1, mono: false, balance: 0, bufferMs: 300, exclusiveFreq: 0, endMode: "stop",
};
const END_MODES = { stop: ["repeat", "Fin de la file : s'arrêter"], repeat: ["repeat", "Répéter la file"], radio: ["radio", "Fin de la file : continuer avec des titres proches"] };
function paintModes() {
  $("shuffle-btn").classList.toggle("on", shuffleOn);
  $("shuffle-btn").title = shuffleOn ? "Aléatoire activé" : "Aléatoire";
  const [ic, label] = END_MODES[audio.endMode] || END_MODES.stop;
  const rb = $("repeat-btn");
  rb.querySelector("use").setAttribute("href", "#i-" + ic);
  rb.title = label;
  rb.classList.toggle("on", audio.endMode !== "stop");
}
function saveAudio() {
  invoke("set_audio", { audio });
  paintModes();
}
function savePlayPrefs() {
  invoke("set_play_prefs", { resume: resumeOn, shuffle: shuffleOn });
  paintModes();
}
$("shuffle-btn").addEventListener("click", () => {
  shuffleOn = !shuffleOn;
  savePlayPrefs();
  toast(shuffleOn ? "Aléatoire activé (sans enchaîner deux fois le même artiste)" : "Aléatoire désactivé");
});
$("repeat-btn").addEventListener("click", () => {
  audio.endMode = { stop: "repeat", repeat: "radio", radio: "stop" }[audio.endMode] || "stop";
  saveAudio();
  toast(END_MODES[audio.endMode][1]);
});

// Titres déjà entendus pendant cette séance (pour la radio).
const heard = [];
/** Fin de la file en mode radio : d'autres titres du même artiste et du même genre. */
function radioNext() {
  const last = trackByPath.get(heard[heard.length - 1]) || queue[queue.length - 1];
  if (!last) return;
  const recent = new Set(heard.slice(-200));
  const ar = nameKey(artistOf(last));
  const genre = (last.genre || "").toLowerCase();
  const pool = tracks.filter((t) => !recent.has(t.path) && (nameKey(artistOf(t)) === ar || (genre && (t.genre || "").toLowerCase() === genre)));
  const pick = smartShuffle(pool.length >= 5 ? pool : tracks.filter((t) => !recent.has(t.path))).slice(0, 40);
  if (pick.length) {
    queue = pick;
    invoke("play", { paths: pick.map((t) => t.path), index: 0 });
    toast("Radio : la suite avec des titres proches");
  }
}

// Reprise au démarrage : la file et la position sont gardées régulièrement.
let lastSaved = "";
function saveSession() {
  if (!resumeOn || !state?.path || !queue.length) return;
  const index = state.index ?? 0;
  const key = state.path + "|" + Math.round(state.position / 5);
  if (key === lastSaved) return;
  lastSaved = key;
  invoke("save_session", { paths: queue.map((t) => t.path), index, position: state.position || 0 });
}
setInterval(saveSession, 10000);
window.addEventListener("beforeunload", saveSession);

// --- navigation ---------------------------------------------------------------------

let route = { view: "music" };
const navStack = [];

function go(r, push = true) {
  const sameMusic = route.view === "music" && r.view === "music" && document.querySelector(".music");
  if (push) navStack.push(route);
  route = r;
  $("back").disabled = navStack.length === 0;
  document.querySelectorAll(".tabs button").forEach((b) => b.classList.toggle("active", b.dataset.view === r.view));
  document.querySelector('.icon-btn[data-view="settings"]').classList.toggle("active", r.view === "settings");
  if (sameMusic) renderDiscography();
  else render();
}

$("back").addEventListener("click", () => {
  if (navStack.length) go(navStack.pop(), false);
});
document.querySelectorAll("[data-view]").forEach((b) =>
  b.addEventListener("click", () => {
    $("search").value = "";
    go(b.dataset.view === "music" ? { view: "music", artist: route.artist } : { view: b.dataset.view });
  })
);

let searchTimer;
$("search").addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    const q = $("search").value.trim();
    if (q) {
      if (route.view === "search") {
        route.q = q;
        render();
      } else go({ view: "search", q });
    } else if (route.view === "search") {
      go(navStack.pop() || { view: "music" }, false);
    }
  }, 150);
});

// --- rendu ------------------------------------------------------------------------------

// Listes de titres affichées, pour retrouver la file au double-clic.
let lists = [];

function render() {
  lists = [];
  const v = $("view");
  if (!tracks.length && route.view !== "settings") {
    v.innerHTML = `<div class="page">${
      scanning
        ? `<div class="empty">Analyse de la bibliothèque…</div>`
        : `<div class="empty">${root ? "Aucune piste trouvée dans ce dossier." : "Aucune bibliothèque pour l'instant."}
            <div class="actions" style="justify-content:center;margin-top:16px"><button class="btn primary" data-go-settings>Choisir le dossier de musique</button></div></div>`
    }</div>`;
    return;
  }
  if (route.view === "music") return renderMusic();
  if (route.view === "playlists") return renderPlaylists();
  const html = { albums: renderAlbums, folders: renderFolders, search: renderSearch, settings: renderSettings }[route.view]();
  v.innerHTML = `<div class="page">${html}</div>`;
  markPlaying();
}

// --- vue Musique ------------------------------------------------------------------------

function renderMusic() {
  if (!route.artist || !artistByName.has(route.artist)) {
    const playing = trackByPath.get(state?.path);
    route.artist = playing ? artistOf(playing) : artists[0]?.name;
  }

  $("view").innerHTML = `<div class="music">
    <section class="pane flat">
      <div class="pane-head">Artistes · ${nf(artists.length)}</div>
      <div class="pane-body artist-list" id="artist-list">${artists
        .map(
          (ar) => `<button class="artist-row" data-artist="${esc(ar.name)}" data-letter-of="${firstLetter(sortName(ar.name))}">
            ${artistImg(ar, "cover round")}
            <span style="min-width:0;display:grid"><span class="name">${esc(ar.name)}</span><span class="count">${plural(ar.albums.length, "album", "albums")}</span></span>
          </button>`
        )
        .join("")}</div>
    </section>
    <section class="pane solo">
      <div class="pane-body" id="discography"></div>
    </section>
    <section class="pane right-pane" id="right-pane"></section>
  </div>`;
  renderDiscography();
  renderRight();
  scrollToChild($("artist-list"), document.querySelector(".artist-row.active"), "center");
}

function renderDiscography() {
  lists = [];
  const ar = artistByName.get(route.artist);
  const el = $("discography");
  if (!el) return;
  document.querySelectorAll(".artist-row").forEach((r) => r.classList.toggle("active", r.dataset.artist === route.artist));
  if (!ar) {
    el.innerHTML = "";
    return;
  }
  const all = ar.albums.flatMap((a) => a.tracks);
  const size = ar.albums.reduce((s, a) => s + a.size, 0);
  const dur = ar.albums.reduce((s, a) => s + a.duration, 0);

  el.innerHTML = `<div class="discography">
    <div class="artist-hero">
      <button class="artist-photo" data-artist-photo="${esc(ar.name)}" title="Changer la photo">${artistImg(ar, "cover round")}</button>
      <div><h1>${esc(ar.name)}</h1>
      <div class="sub">${plural(ar.albums.length, "album", "albums")} · ${plural(ar.trackCount, "titre", "titres")}</div></div>
    </div>
    ${ar.albums.map(albumBlock).join("")}
    <div class="stats">${plural(all.length, "fichier", "fichiers")} · ${fmtSize(size)} · ${fmtTime(dur)}</div>
  </div>`;
  el.scrollTop = 0;
  if (route.album) scrollToChild(el, el.querySelector(`[data-block="${CSS.escape(route.album)}"]`));
  markPlaying();
}

function albumBlock(a) {
  const id = lists.push(a.tracks) - 1;
  const discs = new Map();
  a.tracks.forEach((t, i) => {
    const d = t.disc || 1;
    if (!discs.has(d)) discs.set(d, []);
    discs.get(d).push({ t, i });
  });
  const meta = [a.year, a.genre].filter(Boolean).join(" · ");
  const body = [...discs.entries()]
    .map(([d, rows]) => {
      const half = Math.ceil(rows.length / 2);
      return `<div class="disc-label">Disque ${d}</div>
        <div class="tl" style="grid-template-rows: repeat(${half}, auto)">${rows
          .map(
            ({ t, i }, k) => `<div class="row${(k < half ? k : k - half) % 2 ? " alt" : ""}" data-list="${id}" data-i="${i}" data-path="${esc(t.path)}">
              <span class="n">${t.track ? String(t.track).padStart(2, "0") : ""}</span>
              <span class="ti">${esc(t.title)}</span>
              ${heartBtn(t.path)}
              <span class="d">${fmtTime(t.duration)}</span>
            </div>`
          )
          .join("")}</div>`;
    })
    .join("");
  return `<article class="album-block" data-block="${esc(a.key)}">
    ${img(a.tracks[0].path)}
    <div style="min-width:0">
      <div class="album-head">
        <h2>${esc(a.title)}</h2>
        <button class="play-mini" data-play-album="${esc(a.key)}" aria-label="Lire l'album">${PLAY_SVG}</button>
        <button class="edit-mini" data-album-menu="${esc(a.key)}" aria-label="Gérer l'album" title="Modifier, pochette, correction, renommage">${icon("pencil")}</button>
        ${albumHeart(a)}
        ${qualityBadge(a)}
      </div>
      <div class="album-meta"><span>${esc(meta)}${meta ? " · " : ""}${esc([...a.formats].join(", "))}</span><span class="dur">${fmtLong(a.duration)}</span></div>
      ${body}
    </div>
  </article>`;
}

function renderRight() {
  const el = $("right-pane");
  if (!el) return;
  const cur = trackByPath.get(state?.path);
  const qi = state?.index ?? -1;
  const inQueue = cur && queue[qi]?.path === cur.path;
  const q = inQueue ? queue : cur ? [cur] : [];
  const ci = inQueue ? qi : 0;

  const queueHtml = q
    .map(
      (t, i) => `<button class="q-row ${i === ci ? "current" : i < ci ? "past" : ""}" data-queue="${i}">
        <span class="q-art">${img(t.path)}${i === ci ? EQBARS : ""}</span>
        <span style="min-width:0;display:grid"><span class="t1">${esc(t.title)}</span><span class="t2">${esc(t.artist)}</span></span>
        <span class="d">${fmtTime(t.duration)}</span>
      </button>`
    )
    .join("");

  const fmt = cur
    ? [cur.format + (cur.sampleRate ? ` ${khz(cur.sampleRate)} kHz` : ""), cur.bitDepth ? `${cur.bitDepth} bits` : "", cur.bitrate ? `${nf(cur.bitrate)}k` : "", channels(cur.channels), fmtTime(cur.duration)]
        .filter(Boolean)
        .join(", ")
    : "";

  el.innerHTML = `
    <div class="pane-head">Lecture en cours${q.length ? " · " + plural(q.length, "titre", "titres") : ""}</div>
    <div class="pane-body queue">${queueHtml || `<div class="empty" style="padding:40px 16px">Rien en lecture.</div>`}</div>
    <div class="info">
      ${
        cur
          ? `<div class="info-text">
              <b>${esc(cur.title)}</b>
              <span>${esc(cur.artist)}</span>
              <span>${esc(cur.album)}</span>
              ${cur.year ? `<span>${cur.year}</span>` : ""}
              ${stats.plays[cur.path] ? `<span>${plural(stats.plays[cur.path], "écoute", "écoutes")}</span>` : ""}
              <span class="fmt">${esc(fmt)}</span>
            </div>
            ${img(cur.path)}`
          : `<div class="empty-info">Aucune piste.</div>`
      }
    </div>`;
  scrollToChild(el.querySelector(".queue"), el.querySelector(".q-row.current"), "nearest");
}

// --- vue Playlists ----------------------------------------------------------------------

// --- écoutes : compteurs, historique, playlists automatiques ---------------------------------

let stats = { plays: {}, last: {}, history: [] };
const AUTO = {
  "auto:top": ["Les plus écoutés", "trending"],
  "auto:recent": ["Écoutés récemment", "history"],
  "auto:added": ["Ajoutés récemment", "plus"],
  "auto:never": ["Jamais écoutés", "radio"],
};
function autoPaths(id) {
  const has = (p) => trackByPath.has(p);
  if (id === "auto:top") return Object.entries(stats.plays).filter(([p]) => has(p)).sort((a, b) => b[1] - a[1]).slice(0, 100).map(([p]) => p);
  if (id === "auto:recent") {
    const seen = new Set(), out = [];
    for (let i = stats.history.length - 1; i >= 0 && out.length < 100; i--) {
      const p = stats.history[i][0];
      if (!seen.has(p) && has(p)) {
        seen.add(p);
        out.push(p);
      }
    }
    return out;
  }
  if (id === "auto:added") return tracks.filter((t) => t.added).sort((a, b) => b.added - a.added || collator.compare(a.path, b.path)).slice(0, 200).map((t) => t.path);
  if (id === "auto:never")
    return tracks
      .filter((t) => !stats.plays[t.path])
      .sort((a, b) => collator.compare(artistOf(a), artistOf(b)) || collator.compare(a.album || "", b.album || "") || (a.disc || 1) - (b.disc || 1) || (a.track || 0) - (b.track || 0))
      .slice(0, 500)
      .map((t) => t.path);
  return [];
}
listen("stats-changed", (e) => {
  const [path, n, at] = e.payload;
  stats.plays[path] = n;
  stats.last[path] = at;
  stats.history.push([path, at]);
  if (route.view === "playlists" && String(route.pl).startsWith("auto:")) render();
});

// Écoute : compte quand on a entendu la moitié du titre (ou 4 minutes), comme Last.fm.
let listen_ = { path: null, heard: 0, lastPos: 0, done: false };
function trackListening(s, prev) {
  if (!s.path) return;
  if (s.path !== listen_.path) listen_ = { path: s.path, heard: 0, lastPos: s.position || 0, done: false };
  const pos = s.position || 0;
  const d = pos - listen_.lastPos;
  // Retour au début (répétition) : nouvelle écoute.
  if (d < -5 && pos < 3) listen_ = { path: s.path, heard: 0, lastPos: pos, done: false };
  else if (s.playing && d > 0 && d < 3) listen_.heard += d;
  listen_.lastPos = pos;
  const t = trackByPath.get(s.path);
  const need = Math.min(240, Math.max(30, (s.duration || t?.duration || 0) / 2));
  if (!listen_.done && listen_.heard >= need) {
    listen_.done = true;
    invoke("record_play", { path: s.path, artist: t?.artist || "", title: t?.title || "", album: t?.album || "", duration: s.duration || t?.duration || 0 });
  }
  // Contrôles multimédia de Windows et « en cours d'écoute » Last.fm.
  const changed = s.path !== prev?.path;
  if (changed || s.playing !== prev?.playing || Math.abs((s.position || 0) - (prev?.position || 0)) > 4) {
    invoke("media_update", {
      path: s.path,
      title: t?.title || s.path.split(/[\\/]/).pop(),
      artist: t?.artist || "",
      album: t?.album || "",
      duration: s.duration || 0,
      position: s.position || 0,
      playing: !!s.playing,
      changed,
    });
  }
}

function renderPlaylists() {
  const pls = coll.playlists;
  if (!route.pl || (route.pl !== "liked" && route.pl !== "albums" && !AUTO[route.pl] && !pls.some((p) => p.id === route.pl))) route.pl = "liked";
  const known = (paths) => paths.filter((p) => trackByPath.has(p)).length;
  const likedAlbums = albums.filter((a) => likedA.has(likeKeyOf(a)));
  const item = (id, ic, name, sub) =>
    `<button class="pl-item ${route.pl === id ? "active" : ""}" data-pl="${esc(id)}">
      <span class="pl-ic ${id === "liked" ? "liked" : ""}">${icon(ic)}</span>
      <span style="min-width:0;display:grid"><span class="name">${esc(name)}</span><span class="count">${esc(sub)}</span></span>
    </button>`;
  $("view").innerHTML = `<div class="music pl-layout">
    <section class="pane flat">
      <div class="pane-head">Playlists</div>
      <div class="pane-body pl-list">
        ${item("liked", "heart", "Titres aimés", plural(known(coll.likedTracks), "titre", "titres"))}
        ${item("albums", "heart", "Albums aimés", plural(likedAlbums.length, "album", "albums"))}
        <hr>
        ${Object.entries(AUTO).map(([id, [name, ic]]) => item(id, ic, name, "Automatique")).join("")}
        <hr>
        ${pls.map((p) => item(p.id, "list-music", p.name, plural(p.tracks.length, "titre", "titres"))).join("")}
        <button class="pl-new" data-pl-new>${icon("plus")}<span>Nouvelle playlist</span></button>
      </div>
    </section>
    <section class="pane solo"><div class="pane-body page" id="pl-body">${playlistBody(likedAlbums)}</div></section>
  </div>`;
  const inp = document.querySelector(".pl-rename");
  if (inp) {
    inp.focus();
    inp.select();
  }
  markPlaying();
}

function playlistBody(likedAlbums) {
  if (route.pl === "albums") {
    return `<h1>Albums aimés</h1><p class="status">${plural(likedAlbums.length, "album", "albums")}</p>
      ${likedAlbums.length ? `<div class="grid">${likedAlbums.map(albumCard).join("")}</div>` : `<div class="empty">Touche le cœur à côté du titre d'un album pour le retrouver ici.</div>`}`;
  }
  const auto = AUTO[route.pl];
  const p = route.pl === "liked" || auto ? null : coll.playlists.find((x) => x.id === route.pl);
  const paths = auto ? autoPaths(route.pl) : p ? p.tracks : coll.likedTracks;
  const list = paths.map((path) => trackByPath.get(path) || { path, title: path.split(/[\\/]/).pop(), artist: "", album: "", duration: 0, missing: true });
  const playable = list.filter((t) => !t.missing);
  const id = lists.push(playable) - 1;
  const dur = playable.reduce((x, t) => x + (t.duration || 0), 0);
  let k = -1;
  const rows = list
    .map((t, i) => {
      if (!t.missing) k++;
      return `<tr ${t.missing ? 'class="missing"' : `data-list="${id}" data-i="${k}"`} data-path="${esc(t.path)}" data-pos="${i}" ${p ? 'draggable="true"' : ""}>
        <td class="n">${i + 1}</td>
        <td><div class="pl-ti">${img(t.path, "mini-cover")}<span>${esc(t.title)}</span>${route.pl === "auto:top" && stats.plays[t.path] ? `<span class="plays">${stats.plays[t.path]}×</span>` : ""}</div></td>
        <td>${t.missing ? `<span class="hint">Fichier introuvable</span>` : esc(t.artist)}</td>
        <td>${esc(t.album)}</td>
        <td class="h">${heartBtn(t.path)}</td>
        <td class="t">${t.missing ? "" : fmtTime(t.duration)}</td>
      </tr>`;
    })
    .join("");
  const title = p
    ? route.rename
      ? `<input class="pl-rename" value="${esc(p.name)}" spellcheck="false" aria-label="Nom de la playlist">`
      : `<h1 class="pl-title" data-pl-rename title="Renommer">${esc(p.name)}</h1>`
    : `<h1>${auto ? esc(auto[0]) : "Titres aimés"}</h1>`;
  return `<div class="pl-head">
      <div class="pl-cover ${p || auto ? "" : "liked"}">${auto ? icon(auto[1]) : p ? icon("list-music") : icon("heart", "solid")}</div>
      <div style="min-width:0">
        ${title}
        <p class="status">${plural(playable.length, "titre", "titres")}${dur ? " · " + fmtLong(dur) : ""}</p>
        <div class="actions">
          <button class="btn primary" data-pl-play ${playable.length ? "" : "disabled"}>${icon("play", "solid")}Lire</button>
          <button class="btn" data-pl-shuffle ${playable.length > 1 ? "" : "disabled"}>${icon("shuffle")}Aléatoire</button>
          ${p ? `<button class="btn" data-pl-rename>${icon("pencil")}Renommer</button>
                 <button class="btn" data-pl-delete>${icon("trash")}Supprimer</button>` : ""}
        </div>
      </div>
    </div>
    ${
      list.length
        ? `<table class="tracks pl-tracks"><thead><tr><th class="n">#</th><th>Titre</th><th>Artiste</th><th>Album</th><th class="h"></th><th class="t">Durée</th></tr></thead><tbody>${rows}</tbody></table>`
        : `<div class="empty">${
            auto
              ? route.pl === "auto:added"
                ? "Lance « Tout réanalyser » (Paramètres › Bibliothèque) une fois pour connaître les dates d'ajout."
                : "Rien pour l'instant : les écoutes se comptent à partir de maintenant."
              : p
                ? "Playlist vide. Clic droit sur des titres ou un album › « Ajouter à une playlist »."
                : "Touche le cœur à côté d'un titre pour le retrouver ici."
          }</div>`
    }`;
}

const currentPlaylist = () => coll.playlists.find((x) => x.id === route.pl);
function playlistTracks() {
  const p = currentPlaylist();
  const paths = AUTO[route.pl] ? autoPaths(route.pl) : p ? p.tracks : coll.likedTracks;
  return paths.map((x) => trackByPath.get(x)).filter(Boolean);
}
function shuffled(list) {
  const a = [...list];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}
async function finishRename(inp, save) {
  const p = currentPlaylist();
  route.rename = false;
  if (save && p && inp.value.trim() && inp.value.trim() !== p.name) await collOp({ op: "rename", id: p.id, name: inp.value });
  render();
}

$("view").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-pl],[data-pl-new],[data-pl-play],[data-pl-shuffle],[data-pl-rename],[data-pl-delete]");
  if (!b || b.disabled) return;
  const d = b.dataset;
  if (d.pl) go({ view: "playlists", pl: d.pl }, false);
  else if (d.plNew !== undefined) newPlaylist([], true);
  else if (d.plPlay !== undefined) playList(playlistTracks());
  else if (d.plShuffle !== undefined) playList(shuffled(playlistTracks()));
  else if (d.plRename !== undefined) {
    route.rename = true;
    render();
  } else if (d.plDelete !== undefined) {
    const p = currentPlaylist();
    if (!p) return;
    if (b.dataset.confirm) {
      if (await collOp({ op: "delete", id: p.id })) toast(`Playlist « ${p.name} » supprimée`);
      route.pl = "liked";
      render();
    } else {
      b.dataset.confirm = "1";
      b.classList.add("danger");
      b.lastChild.textContent = "Confirmer la suppression";
      setTimeout(() => b.isConnected && render(), 4000);
    }
  }
});
document.addEventListener("keydown", (e) => {
  const inp = e.target.closest?.(".pl-rename");
  if (!inp) return;
  if (e.key === "Enter") finishRename(inp, true);
  else if (e.key === "Escape") finishRename(inp, false);
  e.stopPropagation();
}, true);
document.addEventListener("focusout", (e) => {
  if (e.target.classList?.contains("pl-rename") && route.rename) finishRename(e.target, true);
});

// Glisser-déposer pour réordonner une playlist.
let dragPos = null;
$("view").addEventListener("dragstart", (e) => {
  const r = e.target.closest?.(".pl-tracks tr[data-pos]");
  if (!r) return;
  dragPos = Number(r.dataset.pos);
  e.dataTransfer.effectAllowed = "move";
  r.classList.add("dragging");
});
$("view").addEventListener("dragover", (e) => {
  const r = e.target.closest?.(".pl-tracks tr[data-pos]");
  if (!r || dragPos === null) return;
  e.preventDefault();
  document.querySelectorAll(".pl-tracks tr.drop-above,.pl-tracks tr.drop-below").forEach((x) => x.classList.remove("drop-above", "drop-below"));
  const box = r.getBoundingClientRect();
  r.classList.add(e.clientY < box.top + box.height / 2 ? "drop-above" : "drop-below");
});
$("view").addEventListener("drop", async (e) => {
  const r = e.target.closest?.(".pl-tracks tr[data-pos]");
  const p = currentPlaylist();
  if (!r || dragPos === null || !p) return;
  e.preventDefault();
  let to = Number(r.dataset.pos) + (r.classList.contains("drop-below") ? 1 : 0);
  if (to > dragPos) to--;
  const from = dragPos;
  dragPos = null;
  if (to !== from) await collOp({ op: "move", id: p.id, from, to });
  else render();
});
$("view").addEventListener("dragend", () => {
  dragPos = null;
  document.querySelectorAll(".pl-tracks tr").forEach((x) => x.classList.remove("dragging", "drop-above", "drop-below"));
});

// Cœurs : un clic suffit, sans sélectionner la ligne.
document.addEventListener(
  "click",
  (e) => {
    const b = e.target.closest("[data-like-track],[data-like-album]");
    if (!b) return;
    e.stopPropagation();
    closeMenu();
    if (b.dataset.likeAlbum) {
      const a = albumByKey.get(b.dataset.likeAlbum);
      if (a) collOp({ op: "likeAlbum", key: likeKeyOf(a), on: !likedA.has(likeKeyOf(a)) });
    } else if (b.dataset.likeTrack) {
      const path = b.dataset.likeTrack;
      collOp({ op: "likeTrack", path, on: !likedT.has(path) });
    }
  },
  true
);

// --- autres vues -------------------------------------------------------------------------

/** Badge de qualité d'un album : « 24/96 » en haute résolution, « MP3 » pour le compressé. */
function qualityBadge(a) {
  if (!ui.badges) return "";
  const lossy = a.tracks.some((t) => /^(mp3|ogg|m4a|aac|opus)$/i.test(t.format || ""));
  if (lossy) return `<span class="q-badge lossy">${esc((a.tracks[0].format || "").toUpperCase())}</span>`;
  const bits = Math.max(...a.tracks.map((t) => t.bitDepth || 0));
  const rate = Math.max(...a.tracks.map((t) => t.sampleRate || 0));
  if (bits > 16 || rate > 48000) return `<span class="q-badge hires" title="Haute résolution">${bits || "?"}/${khz(rate)}</span>`;
  return "";
}

function albumCard(a) {
  return `<div class="card">
    <div class="art">
      <button class="art-open" data-open-album="${esc(a.key)}" aria-label="${esc(a.title)}">${img(a.tracks[0].path)}</button>
      ${qualityBadge(a)}
      <button class="card-play" data-play-album="${esc(a.key)}" aria-label="Lire ${esc(a.title)}">${PLAY_SVG}</button>
    </div>
    <button class="card-title link" data-open-album="${esc(a.key)}">${esc(a.title)}</button>
    <button class="card-sub link" data-artist="${esc(a.artist)}">${esc(a.artist)}</button>
  </div>`;
}

function artistCard(ar) {
  return `<button class="card artist" data-artist="${esc(ar.name)}">
    ${artistImg(ar, "cover round")}
    <span class="card-title">${esc(ar.name)}</span>
    <span class="card-sub">${plural(ar.albums.length, "album", "albums")}</span>
  </button>`;
}

function renderAlbums() {
  return `<h1>Albums</h1><p class="status">${plural(albums.length, "album", "albums")}</p>
    <div class="grid">${albums.map(albumCard).join("")}</div>`;
}

// Recherche : sans accents ni casse, mots dans n'importe quel ordre (titre, artiste, album),
// résultats classés par pertinence (titre exact, puis début de titre, début de mot, etc.).
const searchNorm = (s) => (s || "").normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase("fr").replace(/[^\p{L}\p{N}]+/gu, " ").trim();
const searchCache = new WeakMap();
function searchFields(obj, raw) {
  let f = searchCache.get(obj);
  if (!f) {
    f = raw.map((x) => {
      const n = searchNorm(x);
      return { n, words: n.split(" ") };
    });
    searchCache.set(obj, f);
  }
  return f;
}
/** 0 = pas trouvé ; plus c'est haut, plus c'est pertinent. Le premier champ compte double. */
function searchScore(fields, q, words) {
  let score = 0;
  for (const w of words) {
    let best = 0;
    fields.forEach((f, i) => {
      const weight = i === 0 ? 2 : 1;
      const s = f.words.includes(w) ? 3 : f.words.some((x) => x.startsWith(w)) ? 2 : f.n.includes(w) ? 0.5 : 0;
      best = Math.max(best, s * weight);
    });
    if (!best) return 0;
    score += best;
  }
  const main = fields[0].n;
  if (main === q) score += 20;
  else if (main.startsWith(q)) score += 10;
  else if (main.includes(" " + q)) score += 5;
  return score;
}
function searchRank(list, fieldsOf, q, words, max) {
  return list
    .map((x) => ({ x, s: searchScore(searchFields(x, fieldsOf(x)), q, words) }))
    .filter((r) => r.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, max)
    .map((r) => r.x);
}

function renderSearch() {
  const q = searchNorm(route.q);
  const words = q.split(" ").filter(Boolean);
  const ar = words.length ? searchRank(artists, (x) => [x.name], q, words, 12) : [];
  const al = words.length ? searchRank(albums, (x) => [x.title, x.artist], q, words, 24) : [];
  const tr = words.length ? searchRank(tracks, (t) => [t.title, t.artist, t.album], q, words, 200) : [];
  if (!ar.length && !al.length && !tr.length) return `<div class="empty">Aucun résultat pour « ${esc(route.q)} ».</div>`;
  return `<h1>Résultats pour « ${esc(route.q)} »</h1>
    ${ar.length ? `<h2>Artistes</h2><div class="grid">${ar.map(artistCard).join("")}</div>` : ""}
    ${al.length ? `<h2>Albums</h2><div class="grid">${al.map(albumCard).join("")}</div>` : ""}
    ${tr.length ? `<h2>Titres</h2>${trackTable(tr, { album: true, artist: true })}` : ""}`;
}

function renderFolders() {
  // Plusieurs dossiers de musique : on commence par la liste des dossiers.
  if (!route.dir && extraRoots.length) {
    return `<h1>Dossiers</h1><div class="folders">${allRoots()
      .map((r) => {
        const first = tracks.find((t) => rootOf(t.path) === r);
        const n = tracks.filter((t) => rootOf(t.path) === r).length;
        return `<button class="folder" data-open-dir="${esc(r)}">
          ${first ? img(first.path, "") : ""}
          <span style="min-width:0;display:grid"><span class="name">${esc(r)}</span><span class="count">${plural(n, "titre", "titres")}</span></span>
        </button>`;
      })
      .join("")}</div>`;
  }
  const base = route.dir ? rootOf(route.dir) : root;
  const dir = route.dir || root;
  const prefix = dir + sep(dir);
  const sub = new Map();
  const here = [];
  for (const t of tracks) {
    if (!t.path.startsWith(prefix)) continue;
    const rest = t.path.slice(prefix.length);
    const cut = rest.search(/[\\/]/);
    if (cut < 0) here.push(t);
    else {
      const name = rest.slice(0, cut);
      let f = sub.get(name);
      if (!f) sub.set(name, (f = { name, path: prefix + name, first: t, count: 0 }));
      f.count++;
    }
  }
  const folders = [...sub.values()].sort((a, b) => collator.compare(a.name, b.name));
  const parts = [{ label: base.split(/[\\/]/).filter(Boolean).pop() || base, path: base }];
  if (dir !== base) {
    let acc = base;
    for (const p of dir.slice(base.length + 1).split(/[\\/]/)) {
      acc += sep(base) + p;
      parts.push({ label: p, path: acc });
    }
  }
  const crumbs = parts.map((p) => `<button data-open-dir="${esc(p.path)}">${esc(p.label)}</button>`).join("<span>›</span>");
  return `<div class="crumbs">${crumbs}</div>
    ${folders.length ? `<div class="folders">${folders
      .map(
        (f) => `<button class="folder" data-open-dir="${esc(f.path)}">
          ${img(f.first.path, "")}
          <span style="min-width:0;display:grid"><span class="name">${esc(f.name)}</span><span class="count">${plural(f.count, "titre", "titres")}</span></span>
        </button>`
      )
      .join("")}</div>` : ""}
    ${here.length ? trackTable(sortAlbumTracks(here), { artist: true, album: true }) : ""}
    ${!folders.length && !here.length ? `<div class="empty">Dossier vide.</div>` : ""}`;
}

// =========================================================================================
// Paramètres : navigation par rubriques, une ligne par réglage (titre, explication, contrôle)
// =========================================================================================

const sw = (attrs, on, label = "") => `<label class="switch" ${label ? `title="${esc(label)}"` : ""}><input type="checkbox" ${attrs} ${on ? "checked" : ""}><span></span></label>`;
const srow = (title, desc, control, cls = "") =>
  `<div class="s-row ${cls}"><div class="s-txt"><div class="s-title">${title}</div>${desc ? `<div class="s-desc">${desc}</div>` : ""}</div>${control ? `<div class="s-ctl">${control}</div>` : ""}</div>`;
const scard = (title, body, opts = {}) =>
  `<section class="s-card ${opts.wide ? "wide" : ""}" ${opts.id ? `id="${opts.id}"` : ""}>${title ? `<header class="s-head"><h3>${title}</h3>${opts.action || ""}</header>` : ""}<div class="s-rows">${body}</div></section>`;
const srange = (attrs, value, shown) => `<div class="s-range"><input type="range" ${attrs} value="${value}"><b class="set-val">${shown}</b></div>`;
const ssel = (attrs, options) => `<select class="s-sel" ${attrs}>${options}</select>`;

const SETTINGS_NAV = [
  ["Écoute", [
    ["lecture", "Lecture", "play"],
    ["son", "Son et volume", "sliders-vertical"],
    ["sortie", "Sortie audio", "speaker"],
  ]],
  ["Bibliothèque", [
    ["dossiers", "Dossiers", "folder"],
    ["classement", "Classement et fichiers", "list-music"],
    ["historique", "Historique des modifications", "history"],
  ]],
  ["Interface", [
    ["apparence", "Apparence", "palette"],
    ["ambiance", "Mode ambiance", "maximize"],
    ["raccourcis", "Raccourcis clavier", "keyboard"],
  ]],
  ["Connexions", [
    ["telephone", "Téléphone", "smartphone"],
    ["services", "Services en ligne", "globe"],
  ]],
  ["Système", [
    ["systeme", "Windows et sauvegarde", "settings"],
  ]],
];
const SETTINGS_PAGES = {
  lecture: ["Lecture", "Comment les morceaux s'enchaînent, et ce qui se passe à la fin de la file.", pageLecture],
  son: ["Son et volume", "Niveau des morceaux, limites et réglages du signal.", pageSon],
  sortie: ["Sortie audio", "Le périphérique qui joue la musique, et la façon dont le son lui est envoyé.", pageSortie],
  dossiers: ["Dossiers", "Où se trouve ta musique.", pageDossiers],
  classement: ["Classement et fichiers", "Comment la bibliothèque est rangée, ici et sur le disque.", pageClassement],
  historique: ["Historique des modifications", "Les changements faits sur tes fichiers, annulables.", () => historySection()],
  apparence: ["Apparence", "Thème, couleur et affichage.", pageApparence],
  ambiance: ["Mode ambiance", "Plein écran avec la pochette, quand tu n'es pas devant l'ordinateur.", pageAmbiance],
  raccourcis: ["Raccourcis clavier", "", () => shortcutsSection()],
  telephone: ["Téléphone", "L'app Android : écoute, télécommande et albums hors ligne.", () => phoneSection()],
  services: ["Services en ligne", "Pochettes, fiches d'albums et écoutes partagées.", pageServices],
  systeme: ["Windows et sauvegarde", "Démarrage, zone de notification, et sauvegarde de tes données.", pageSysteme],
};

function settingsMain() {
  const [title, sub, fn] = SETTINGS_PAGES[settingsTab];
  return `<header class="s-page-head"><h1>${esc(title)}</h1>${sub ? `<p>${esc(sub)}</p>` : ""}</header>
      <div class="s-grid">${fn()}</div>`;
}

function renderSettings() {
  if (!SETTINGS_PAGES[settingsTab]) settingsTab = "lecture";
  return `<div class="settings-page">
    <nav class="s-nav">
      <div class="s-nav-title">Paramètres</div>
      ${SETTINGS_NAV.map(
        ([group, items]) => `<div class="s-group">${esc(group)}</div>${items
          .map(([id, label, ic]) => `<button data-settings-tab="${id}" class="${id === settingsTab ? "active" : ""}">${icon(ic)}<span>${esc(label)}</span></button>`)
          .join("")}`
      ).join("")}
    </nav>
    <div class="s-main">${settingsMain()}</div>
  </div>`;
}

// --- Écoute ------------------------------------------------------------------------------

function pageLecture() {
  return (
    scard(
      "Enchaînement",
      srow("Fondu enchaîné", "Le morceau suivant commence pendant que celui-ci s'efface.", srange('min="0" max="10" step="0.5" data-audio="crossfade" data-fmt="sec"', audio.crossfade, fmtSec(audio.crossfade))) +
        srow("Fondre aussi dans un même album", "Sinon, les titres d'un album s'enchaînent sans blanc, comme sur le disque (live, albums concepts).", sw('data-audio="crossfadeAlbums"', audio.crossfadeAlbums)) +
        srow("Fondu à la pause et à la reprise", "Évite le petit claquement quand on coupe le son en plein morceau.", ssel('data-audio="fadeMs"', opt(0, audio.fadeMs, "Aucun") + opt(80, audio.fadeMs, "Très court") + opt(150, audio.fadeMs, "Court") + opt(300, audio.fadeMs, "Doux") + opt(600, audio.fadeMs, "Lent")))
    ) +
    scard(
      "File de lecture",
      srow("Aléatoire", "Mélange la file sans enchaîner deux titres du même artiste quand c'est possible.", sw('id="shuffle-on"', shuffleOn)) +
        srow("À la fin de la file", "Le bouton répétition du lecteur fait la même chose.", ssel('data-audio="endMode"', opt("stop", audio.endMode, "S'arrêter") + opt("repeat", audio.endMode, "Recommencer") + opt("radio", audio.endMode, "Continuer avec des titres proches"))) +
        srow("Reprendre au démarrage", "Retrouver la file et le morceau là où tu t'étais arrêté.", sw('id="resume-on"', resumeOn))
    )
  );
}

function pageSon() {
  return (
    scard(
      "Volume des morceaux (ReplayGain)",
      srow("Égalisation du volume", "Met tous les morceaux au même niveau sonore grâce aux infos ReplayGain des fichiers. « Par album » garde les écarts voulus entre les titres d'un album.", ssel('data-audio="replaygain"', opt("off", audio.replaygain, "Désactivée") + opt("track", audio.replaygain, "Par titre") + opt("album", audio.replaygain, "Par album"))) +
        srow("Pré-amplification", "Pour remonter ou baisser l'ensemble.", srange('min="-12" max="12" step="0.5" data-audio="rgPreamp" data-fmt="db"', audio.rgPreamp, fmtDbSigned(audio.rgPreamp))) +
        srow("Ne jamais saturer", "Limite le gain à la crête du fichier.", sw('data-audio="rgClip"', audio.rgClip))
    ) +
    scard(
      "Limites",
      srow("Volume maximum", "Le curseur de volume ne peut pas dépasser cette limite (utile en mode exclusif).", srange('min="10" max="100" step="5" data-audio="maxVolume" data-fmt="pct"', Math.round(audio.maxVolume * 100), Math.round(audio.maxVolume * 100) + " %")) +
        srow("Mono", "Les deux canaux mélangés.", sw('data-audio="mono"', audio.mono)) +
        srow("Balance", "", srange('min="-1" max="1" step="0.05" data-audio="balance" data-fmt="bal"', audio.balance, fmtBal(audio.balance)) + `<button class="btn small" data-balance-reset>Centrer</button>`)
    ) +
    scard(
      "Égaliseur",
      srow("Égaliseur 10 bandes", "Préréglages, tes propres réglages, un réglage par sortie et import AutoEQ.", `<button class="btn" data-open-eq>${icon("sliders-vertical")}Ouvrir l'égaliseur</button>`)
    )
  );
}

function pageSortie() {
  // Liste des périphériques remplie à l'affichage.
  setTimeout(fillSettingsDevices);
  return (
    scard(
      "Périphérique",
      srow("Sortie", "« Défaut de Windows » suit le périphérique choisi dans Windows.", `<select class="s-sel" id="set-device"><option value="">Chargement…</option></select>`) +
        srow("Mode", "Partagé : le son passe par le mixeur de Windows (volume Windows actif). Exclusif : l'app parle seule au DAC, en bit-perfect.", ssel('id="set-mode"', opt("shared", output.exclusive ? "exclusive" : "shared", "Partagé") + opt("exclusive", output.exclusive ? "exclusive" : "shared", "Exclusif (bit-perfect)")))
    ) +
    scard(
      "Avancé",
      srow("Tampon de sortie", "Plus court : réactions plus rapides. Plus long : plus stable si le son craque.", ssel('data-audio="bufferMs"', [100, 200, 300, 500, 800].map((v) => opt(v, audio.bufferMs, v + " ms" + (v === 300 ? " (conseillé)" : ""))).join(""))) +
        srow("Fréquence en mode exclusif", "Imposer une fréquence rééchantillonne ce qui n'y est pas déjà.", ssel('data-audio="exclusiveFreq"', opt(0, audio.exclusiveFreq, "Celle du fichier (bit-perfect)") + [44100, 48000, 88200, 96000, 176400, 192000].map((f) => opt(f, audio.exclusiveFreq, khz(f) + " kHz")).join("")))
    )
  );
}
async function fillSettingsDevices() {
  const el = $("set-device");
  if (!el) return;
  const list = await invoke("list_devices");
  el.innerHTML = `<option value="">Défaut de Windows</option>` + list.map((d) => `<option value="${esc(d.id)}">${esc(d.name)}${d.isDefault ? " (défaut)" : ""}</option>`).join("");
  el.value = output.deviceId ?? "";
}
document.addEventListener("change", (e) => {
  if (e.target.id !== "set-device" && e.target.id !== "set-mode") return;
  output = { deviceId: $("set-device").value || null, exclusive: $("set-mode").value === "exclusive" };
  invoke("set_output", { output });
  // Le panneau du haut suit.
  const r = document.querySelector(`input[name="mode"][value="${output.exclusive ? "exclusive" : "shared"}"]`);
  if (r) r.checked = true;
  toast(output.exclusive ? "Mode exclusif : pense à régler le volume sur le DAC ou l'ampli" : "Sortie modifiée");
});
document.addEventListener("change", (e) => {
  if (e.target.id !== "ui-lang") return;
  const v = e.target.value;
  try {
    localStorage.setItem("lang", v);
  } catch {}
  ui.lang = v;
  invoke("set_ui", { ui }).finally(() => location.reload());
});
document.addEventListener("click", (e) => {
  if (e.target.closest("[data-open-eq]")) {
    e.stopPropagation();
    toggleEqPanel(true);
  }
});

// --- Bibliothèque ----------------------------------------------------------------------------

function pageDossiers() {
  const info = scanning
    ? scanProgress
    : root
      ? `${plural(tracks.length, "titre", "titres")} · ${plural(albums.length, "album", "albums")} · ${plural(artists.length, "artiste", "artistes")}`
      : "";
  const FORMATS = ["mp3", "wav", "aiff", "aif", "ogg", "m4a"];
  const roots = allRoots();
  return (
    scard(
      "Dossiers de musique",
      (roots.length
        ? roots
            .map((r, i) =>
              srow(
                `<span class="s-path">${esc(r)}</span>`,
                i === 0 ? "Dossier principal" : "",
                `<button class="btn small" data-remove-root="${esc(r)}" ${scanning ? "disabled" : ""}>Retirer</button>`
              )
            )
            .join("")
        : srow("Aucun dossier choisi", "Choisis le dossier où se trouve ta musique.", "")) +
        `<div class="s-row s-actions"><span id="lib-info" class="s-desc">${esc(info)}</span><div class="s-ctl">
          <button class="btn primary" data-add-root ${scanning ? "disabled" : ""}>${icon("plus")}${root ? "Ajouter un dossier" : "Choisir un dossier"}</button>
          ${root ? `<button class="btn" data-rescan ${scanning ? "disabled" : ""}>Tout réanalyser</button>` : ""}
        </div></div>`,
      { wide: true }
    ) +
    scard(
      "Mises à jour",
      srow("Détection automatique", "Les albums ajoutés, déplacés ou supprimés apparaissent tout seuls (vérification toutes les 30 s et au démarrage).", sw('id="lib-watch"', libPrefs.watch))
    ) +
    scard(
      "Formats",
      srow("Formats à ignorer", "Ils n'apparaissent plus dans l'app mais restent sur le disque. Le FLAC est toujours lu.", `<div class="chips">${FORMATS.map((f) => `<label class="chip"><input type="checkbox" data-ignore-ext="${f}" ${libPrefs.ignoreExt.includes(f) ? "checked" : ""}><span>${f.toUpperCase()}</span></label>`).join("")}</div>`, "stack")
    )
  );
}

function pageClassement() {
  return (
    scard(
      "Classement",
      srow("Ignorer les articles", "« The Cure » rangé à C, « Les Négresses Vertes » à N.", sw('data-ui="ignoreArticles"', ui.ignoreArticles)) +
        srow("Compilations", "Albums à plusieurs artistes, ou « Various Artists ».", ssel('data-ui="compilations"', opt("various", ui.compilations, "Sous « Artistes divers »") + opt("artist", ui.compilations, "Par artiste de chaque titre")))
    ) +
    scard(
      "Fichiers",
      srow("Pochette dans le dossier", "Enregistrer aussi la pochette en « cover.jpg » à côté des fichiers.", sw('id="cover-file"', settings.coverFile)) +
        srow("Modèle de rangement", `Utilisé par « Renommer les fichiers ». Champs : ${TOKENS.map((k) => `<code>{${k}}</code>`).join(" ")}`, `<input class="s-input" id="default-pattern" value="${esc(settings.renamePattern)}" spellcheck="false">`, "stack")
    )
  );
}

// --- Interface ---------------------------------------------------------------------------------

function pageApparence() {
  return (
    scard(
      "Thème",
      `<div class="themes">${MODES.map(
        ([id, label, bg, ink]) => `<button class="theme-card ${id === mode ? "active" : ""}" data-theme-pick="${id}">
          <span class="swatch" style="background:${bg}"><i style="background:${ink}"></i><i style="background:var(--accent)"></i></span>
          <span>${esc(label)}</span>
        </button>`
      ).join("")}</div>`
    ) +
    scard(
      "Couleur d'accent",
      `<div class="themes accents">
        <button class="theme-card ${accentPref === "coral" ? "active" : ""}" data-accent-pick="coral"><span class="swatch" style="background:#FF5533"></span><span>Corail</span></button>
        <button class="theme-card ${accentPref === "windows" ? "active" : ""}" data-accent-pick="windows"><span class="swatch" style="background:${esc(winAccent?.accent || "linear-gradient(135deg,#0078D4,#8A2BE2)")}"></span><span>Couleur de Windows</span></button>
      </div><p class="s-desc s-pad">« Couleur de Windows » reprend l'accent de Paramètres Windows › Personnalisation › Couleurs, dans une nuance adaptée au mode clair ou sombre.</p>`
    ) +
    scard(
      "Affichage",
      srow("Langue", "", ssel('id="ui-lang"', opt("auto", LANG_PREF, "Automatique") + opt("fr", LANG_PREF, "Français") + opt("en", LANG_PREF, "English"))) +
        srow("Taille", "Agrandit ou réduit toute l'interface.", ssel('data-ui="zoom"', [85, 90, 100, 110, 125, 140].map((z) => opt(z, ui.zoom, z + " %" + (z === 100 ? " (normale)" : ""))).join(""))) +
        srow("Densité des listes", "", ssel('data-ui="density"', opt("compact", ui.density, "Compacte") + opt("normal", ui.density, "Normale") + opt("airy", ui.density, "Aérée"))) +
        srow("Vue à l'ouverture", "", ssel('data-ui="startView"', opt("music", ui.startView, "Musique") + opt("albums", ui.startView, "Albums") + opt("folders", ui.startView, "Dossiers") + opt("playlists", ui.startView, "Playlists"))) +
        srow("Badges de qualité", "« 24/96 » sur les albums en haute résolution, « MP3 » sur les fichiers compressés.", sw('data-ui="badges"', ui.badges)),
      { wide: false }
    )
  );
}

function pageAmbiance() {
  const opts = [60, 120, 300, 600, 900, 1800];
  const label = (v) => (v < 120 ? "1 minute" : `${v / 60} minutes`);
  return (
    scard(
      "Affichage",
      srow("Ouvrir", `Bouton ${icon("maximize")} dans le lecteur, ou ${comboLabel(shortcuts.ambient)}. Échap pour revenir.`, `<button class="btn primary" data-open-ambient>${icon("maximize")}Lancer</button>`) +
        srow("Fond", "", ssel('data-ui="ambientStyle"', opt("cover", ui.ambientStyle, "Pochette floutée et couleurs") + opt("color", ui.ambientStyle, "Couleurs de l'album") + opt("dark", ui.ambientStyle, "Sombre et sobre"))) +
        srow("Horloge", "", sw('data-ui="ambientClock"', ui.ambientClock))
    ) +
    scard(
      "Lancement automatique",
      srow("Pendant la lecture", "Quand personne n'a touché l'ordinateur depuis un moment (toutes applications confondues).", sw('id="amb-auto"', ambientSettings.auto)) +
        srow("Après", "", `<select class="s-sel amb-delay" id="amb-delay">${opts.map((v) => opt(v, ambientSettings.delay, label(v))).join("")}</select>`)
    )
  );
}
document.addEventListener("click", (e) => {
  if (e.target.closest("[data-open-ambient]")) openAmbient();
});

// --- Connexions ------------------------------------------------------------------------------

function pageServices() {
  const lf = settings.lastfm || {};
  return (
    scard(
      "Discogs",
      srow("Jeton personnel", "À créer sur discogs.com › Settings › Developers › « Generate new token ». Il sert aux pochettes, fiches d'albums et photos d'artistes. MusicBrainz, iTunes et Deezer marchent sans compte.", `<input class="s-input" id="discogs-token" type="password" value="${esc(settings.discogsToken)}" placeholder="Colle ici ton jeton" spellcheck="false" autocomplete="off">`, "stack")
    ) +
    scard(
      "Last.fm",
      lf.session
        ? srow(`Connecté en tant que <b>${esc(lf.user)}</b>`, "", `<button class="btn small" data-lf-logout>Se déconnecter</button>`) +
            srow("Envoyer mes écoutes", "Scrobbles et morceau en cours.", sw('id="lf-scrobble"', lf.scrobble))
        : `<p class="s-desc s-pad">Crée d'abord une clé d'API gratuite sur last.fm/api/account/create (nom : MusicPi, le reste peut rester vide), puis remplis ces champs. Le mot de passe ne sert qu'une fois et n'est pas enregistré.</p>
           <div class="s-form">
             <label class="fld"><span>Clé d'API</span><input id="lf-key" value="${esc(lf.apiKey || "")}" spellcheck="false" autocomplete="off"></label>
             <label class="fld"><span>Secret partagé</span><input id="lf-secret" type="password" value="${esc(lf.apiSecret || "")}" spellcheck="false" autocomplete="off"></label>
             <label class="fld"><span>Nom d'utilisateur</span><input id="lf-user" value="${esc(lf.user || "")}" spellcheck="false" autocomplete="off"></label>
             <label class="fld"><span>Mot de passe</span><input id="lf-pass" type="password" autocomplete="off"></label>
           </div>
           <div class="s-row s-actions"><span></span><div class="s-ctl"><button class="btn primary" data-lf-login>Se connecter</button></div></div>`
    )
  );
}

// --- Système ---------------------------------------------------------------------------------------

function pageSysteme() {
  return (
    scard(
      "Windows",
      srow("Lancer au démarrage", "Discrètement, dans la zone de notification.", sw('id="sys-autostart"', sysPrefs.autostart)) +
        srow("Fermer = réduire", "Fermer la fenêtre la range dans la zone de notification ; la musique continue.", sw('id="sys-tray"', sysPrefs.trayClose)) +
        srow("Contrôles multimédia", sysPrefs.smtc ? "Titre et pochette dans le panneau du volume et sur l'écran verrouillé ; touches multimédia du clavier." : "Indisponibles : les touches multimédia restent gérées directement.", `<span class="s-badge ${sysPrefs.smtc ? "ok" : ""}">${sysPrefs.smtc ? "Actifs" : "Inactifs"}</span>`)
    ) +
    scard(
      "Sauvegarde",
      srow("Réglages, cœurs, playlists et écoutes", "Dans un fichier, pour réinstaller ou retrouver tout ça sur un autre PC. La musique n'est pas dedans.", `<button class="btn primary" data-backup-export>Exporter…</button><button class="btn" data-backup-import>Importer…</button>`)
    )
  );
}

let settingsTab = "lecture";
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-settings-tab]");
  if (!b) return;
  settingsTab = b.dataset.settingsTab;
  if (settingsTab === "historique") historyCache = null;
  if (settingsTab === "telephone") phoneCache = null;
  // Seul le contenu change : la navigation reste en place, le contenu apparaît en fondu.
  const main = document.querySelector(".s-main");
  if (!main) return render();
  document.querySelectorAll(".s-nav [data-settings-tab]").forEach((x) => x.classList.toggle("active", x.dataset.settingsTab === settingsTab));
  lists = [];
  main.innerHTML = settingsMain();
  main.scrollTop = 0;
  main.classList.remove("swap");
  void main.offsetWidth;
  main.classList.add("swap");
});

function trackTable(list, opts = {}) {
  const id = lists.push(list) - 1;
  const rows = list
    .map(
      (t, i) => `<tr data-list="${id}" data-i="${i}" data-path="${esc(t.path)}">
        <td class="n">${t.track ?? ""}</td>
        <td>${esc(t.title)}</td>
        ${opts.artist ? `<td>${esc(t.artist)}</td>` : ""}
        ${opts.album ? `<td>${esc(t.album)}</td>` : ""}
        <td class="h">${heartBtn(t.path)}</td>
        <td class="fmt">${esc(fmtLabel(t))}</td>
        <td class="t">${fmtTime(t.duration)}</td>
      </tr>`
    )
    .join("");
  return `<table class="tracks"><thead><tr>
      <th class="n">#</th><th>Titre</th>${opts.artist ? "<th>Artiste</th>" : ""}${opts.album ? "<th>Album</th>" : ""}
      <th class="h"></th><th class="fmt">Format</th><th class="t">Durée</th>
    </tr></thead><tbody>${rows}</tbody></table>`;
}

function markPlaying() {
  paintSelection();
  hydrateArtistPhotos();
  const p = state?.path;
  document.querySelectorAll("[data-path]").forEach((r) => {
    const on = r.dataset.path === p;
    r.classList.toggle("playing", on);
    const n = r.querySelector(".n");
    if (n && r.classList.contains("row")) {
      if (on) n.innerHTML = EQBARS;
      else if (n.querySelector(".eqbars")) {
        const t = trackByPath.get(r.dataset.path);
        n.textContent = t?.track ? String(t.track).padStart(2, "0") : "";
      }
    }
  });
  const cur = trackByPath.get(p);
  document.querySelectorAll(".artist-row").forEach((r) => r.classList.toggle("playing", !!cur && r.dataset.artist === artistOf(cur)));
}

// --- clics ------------------------------------------------------------------------------

$("view").addEventListener("click", (e) => {
  const el = e.target.closest(
    "[data-artist-photo],[data-album-menu],[data-theme-pick],[data-shortcut],[data-shortcut-clear],[data-shortcuts-reset],[data-play-album],[data-open-album],[data-artist],[data-open-dir],[data-go-settings],[data-pick-folder],[data-rescan],[data-queue],[data-i]"
  );
  if (!el || el.disabled) return;
  const d = el.dataset;
  if (d.artistPhoto) {
    const ar = artistByName.get(d.artistPhoto);
    if (ar) openEditor(ar.albums.flatMap((a) => a.tracks), { tab: "photo", title: ar.name, artist: ar.name });
    return;
  }
  if (d.albumMenu) {
    const a = albumByKey.get(d.albumMenu);
    const r = el.getBoundingClientRect();
    if (a) setTimeout(() => showMenu(r.left, r.bottom + 6, albumMenuItems(a)));
    return;
  }
  if (d.themePick) {
    applyMode(d.themePick);
    invoke("set_theme", { theme: mode });
    document.querySelectorAll(".theme-card[data-theme-pick]").forEach((c) => c.classList.toggle("active", c.dataset.themePick === mode));
    return;
  }
  if (d.shortcut) {
    captureFor = d.shortcut;
    renderShortcutRows();
    return;
  }
  if (d.shortcutClear) {
    shortcuts[d.shortcutClear] = "";
    saveShortcuts();
    return;
  }
  if (d.shortcutsReset !== undefined) {
    shortcuts = { ...DEFAULT_SHORTCUTS };
    saveShortcuts();
    return;
  }
  if (d.goSettings !== undefined) {
    settingsTab = "dossiers";
    go({ view: "settings" });
  }
  else if (d.pickFolder !== undefined) pickFolder();
  else if (d.rescan !== undefined) root && scan();
  else if (d.playAlbum) playList(albumByKey.get(d.playAlbum).tracks);
  else if (d.openAlbum) {
    const a = albumByKey.get(d.openAlbum);
    go({ view: "music", artist: a.artist, album: a.key });
  } else if (d.artist) {
    if (route.view !== "music" || route.artist !== d.artist) go({ view: "music", artist: d.artist });
  } else if (d.openDir !== undefined) go({ view: "folders", dir: d.openDir });
  else if (d.queue !== undefined) playList(queue, Number(d.queue));
  else if (d.i !== undefined) selectRow(el, e);
});

$("view").addEventListener("dblclick", (e) => {
  if (e.target.closest(".heart")) return;
  const r = e.target.closest("[data-list][data-i]");
  if (r) playList(lists[Number(r.dataset.list)], Number(r.dataset.i));
});

// --- bibliothèque -----------------------------------------------------------------------

let scanning = false;
let scanProgress = "";

function setLibrary(list) {
  tracks = list;
  index();
  render();
}

async function scan(folder) {
  if (folder) root = folder;
  scanning = true;
  scanProgress = "Analyse du dossier…";
  render();
  let list = tracks;
  try {
    list = await invoke("scan_library", { folder: folder || null });
  } finally {
    scanning = false;
  }
  setLibrary(list);
}

// Préférences de la bibliothèque (surveillance, formats ignorés).
let libPrefs = { watch: true, ignoreExt: [] };
document.addEventListener("click", async (e) => {
  const add = e.target.closest("[data-add-root]");
  const rm = e.target.closest("[data-remove-root]");
  if (add) {
    const folder = await invoke("pick_folder");
    if (!folder) return;
    if (!root) return scan(folder);
    if (allRoots().includes(folder)) return toast("Ce dossier est déjà dans la bibliothèque.");
    await invoke("add_library_folder", { folder });
    extraRoots.push(folder);
    scan();
  } else if (rm) {
    const folder = rm.dataset.removeRoot;
    await invoke("remove_library_folder", { folder });
    const st = await invoke("get_settings");
    root = st.libraryFolder;
    extraRoots = st.extraFolders || [];
    if (root) scan();
    else setLibrary([]);
  }
});
document.addEventListener("change", (e) => {
  const el = e.target;
  if (el.id === "lib-watch" || el.dataset?.ignoreExt) {
    libPrefs = {
      watch: $("lib-watch").checked,
      ignoreExt: [...document.querySelectorAll("[data-ignore-ext]:checked")].map((x) => x.dataset.ignoreExt),
    };
    invoke("set_library_prefs", libPrefs);
    if (el.dataset?.ignoreExt && root) scan();
    return;
  }
  if (el.dataset?.ui) {
    const k = el.dataset.ui;
    ui[k] = el.type === "checkbox" ? el.checked : el.type === "range" || el.type === "number" ? Number(el.value) : el.value;
    saveUi();
    applyUi();
    if (k === "ignoreArticles" || k === "compilations") {
      index();
      render();
    }
  }
});
// Changements détectés dans les dossiers.
listen("library-changed", (e) => {
  const ch = e.payload;
  const before = albums.length;
  mergeChanged(ch);
  const diff = albums.length - before;
  if (diff > 0) toast(`${plural(diff, "nouvel album", "nouveaux albums")} dans la bibliothèque`);
  else if (ch.tracks?.length || ch.removed?.length) toast("Bibliothèque mise à jour");
});

/** Applique la taille d'affichage et la densité. */
function applyUi() {
  document.documentElement.style.zoom = String((ui.zoom || 100) / 100);
  document.documentElement.dataset.density = ui.density || "normal";
}

listen("scan-progress", (e) => {
  const [done, total] = e.payload;
  if (!total) return;
  scanProgress = `Analyse… ${nf(done)} / ${nf(total)} fichiers`;
  const el = $("lib-info");
  if (el) el.textContent = scanProgress;
});

async function pickFolder() {
  const folder = await invoke("pick_folder");
  if (folder) scan(folder);
}

// --- lecteur ---------------------------------------------------------------------------

$("play").addEventListener("click", () => invoke("toggle"));
$("prev").addEventListener("click", () => invoke("prev"));
$("next").addEventListener("click", () => invoke("next"));
$("np-artist").addEventListener("click", () => {
  const t = trackByPath.get(state?.path);
  if (t) go({ view: "music", artist: artistOf(t) });
});
$("np-cover").addEventListener("click", () => {
  const t = trackByPath.get(state?.path);
  if (t) go({ view: "music", artist: artistOf(t), album: albumKeyOf(t) });
});

let seeking = false;
const seekEl = $("seek");
seekEl.addEventListener("input", () => {
  seeking = true;
  $("pos").textContent = fmtTime((seekEl.value / 1000) * (state?.duration || 0));
  seekEl.style.setProperty("--p", seekEl.value / 10 + "%");
});
seekEl.addEventListener("change", () => {
  invoke("seek", { position: (seekEl.value / 1000) * (state?.duration || 0) });
  setTimeout(() => (seeking = false), 300);
});

const paintVolume = () => {
  const v = Number($("volume").value);
  $("volume").style.setProperty("--v", v + "%");
  $("vol-ic").querySelector("use").setAttribute("href", v === 0 ? "#i-volume-x" : v < 50 ? "#i-volume-1" : "#i-volume-2");
};
$("volume").addEventListener("input", (e) => {
  paintVolume();
  invoke("set_volume", { volume: e.target.value / 100 });
});


listen("player-state", (e) => {
  const prev = state;
  const s = (state = e.payload);
  if (ambientOpen) ambientUpdate(s, prev);
  if (s.path && s.path !== prev?.path) heard.push(s.path);
  if (s.signal?.device) eqFollowDevice(s.signal.device);
  trackListening(s, prev);
  if (s.finished && !prev?.finished && audio.endMode === "radio") radioNext();

  document.body.classList.toggle("is-paused", !s.playing);
  // Bouton lecture/pause : bascule animée des deux icônes + onde au changement.
  const pb = $("play");
  if (pb.classList.contains("playing") !== !!s.playing) {
    pb.classList.toggle("playing", !!s.playing);
    if (prev) {
      pb.classList.remove("pulse");
      void pb.offsetWidth;
      pb.classList.add("pulse");
    }
  }

  if (s.path !== prev?.path || s.index !== prev?.index) {
    const t = trackByPath.get(s.path);
    // Petit fondu sur le bloc « en lecture » à chaque changement de piste.
    const now = document.querySelector(".player .now");
    now.classList.remove("swap");
    void now.offsetWidth;
    now.classList.add("swap");
    $("np-title").textContent = t ? t.title : s.path ? s.path.split(/[\\/]/).pop() : "Aucune lecture";
    $("np-artist").textContent = t ? t.artist : "";
    const c = $("np-cover");
    c.hidden = !s.path;
    if (s.path) {
      c.onerror = () => (c.src = BLANK);
      c.src = coverUrl(s.path);
    }
    markPlaying();
    renderRight();
    paintHearts();
  }

  if (!seeking) {
    const p = s.duration ? (s.position / s.duration) * 1000 : 0;
    seekEl.value = p;
    seekEl.style.setProperty("--p", p / 10 + "%");
    $("pos").textContent = fmtTime(s.position);
  }
  $("dur").textContent = fmtTime(s.duration);
  syncPractice(s, prev);
  if (document.activeElement !== $("volume")) $("volume").value = Math.round(s.volume * 100);
  paintVolume();

  const sig = $("signal");
  if (s.signal) {
    sig.hidden = false;
    sig.classList.toggle("ok", s.signal.bitPerfect);
    sig.classList.toggle("shared", s.signal.shared);
    const label = s.signal.shared ? "Windows · " : s.signal.bitPerfect ? "Bit-perfect · " : "Converti · ";
    $("signal-label").textContent = label + s.signal.source;
    $("signal-tip").innerHTML = `
      <div><span class="k">Source</span><br><b>${esc(s.signal.source)}</b></div>
      <div><span class="k">Sortie</span><br><b>${esc(s.signal.output)}</b></div>
      <div><span class="k">Périphérique</span><br><b>${esc(s.signal.device)}</b></div>
      ${s.signal.notes.length ? `<ul>${s.signal.notes.map((n) => `<li>${esc(n)}</li>`).join("")}</ul>` : ""}`;
  } else if (s.error) {
    sig.hidden = false;
    sig.classList.remove("ok", "shared");
    $("signal-label").textContent = "Erreur";
    $("signal-tip").innerHTML = `<div class="error">${esc(s.error)}</div>`;
  } else {
    sig.hidden = true;
  }
});

// --- sortie audio ------------------------------------------------------------------------

const panel = $("out-panel");
let output = { deviceId: null, exclusive: false };

async function fillDevices() {
  const list = await invoke("list_devices");
  $("device").innerHTML =
    `<option value="">Périphérique par défaut de Windows</option>` +
    list.map((d) => `<option value="${esc(d.id)}">${esc(d.name)}${d.isDefault ? " (défaut)" : ""}</option>`).join("");
  $("device").value = output.deviceId ?? "";
}

$("out-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  panel.hidden = !panel.hidden;
  if (!panel.hidden) fillDevices();
});
document.addEventListener("click", (e) => {
  if (!panel.hidden && !panel.contains(e.target)) panel.hidden = true;
});

function applyOutput() {
  output = {
    deviceId: $("device").value || null,
    exclusive: document.querySelector('input[name="mode"]:checked').value === "exclusive",
  };
  invoke("set_output", { output });
}
$("device").addEventListener("change", applyOutput);
document.querySelectorAll('input[name="mode"]').forEach((r) => r.addEventListener("change", applyOutput));

// --- égaliseur ------------------------------------------------------------------------

const EQ_FREQS = ["32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
const EQ_PRESETS = {
  "Plat": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  "Basses +": [6, 5, 4, 2, 0.5, 0, 0, 0, 0, 0],
  "Basses −": [-6, -5, -4, -2, -0.5, 0, 0, 0, 0, 0],
  "Aigus +": [0, 0, 0, 0, 0, 0.5, 2, 4, 5, 6],
  "Aigus −": [0, 0, 0, 0, 0, -0.5, -2, -4, -5, -6],
  "Rock": [4.5, 3.5, 2, -1, -2, -1, 1.5, 3, 4, 4.5],
  "Pop": [-1, 1, 3, 4, 3, 0, -1, -1, 0, 1],
  "Jazz": [3, 2, 1, 2, -1.5, -1.5, 0, 1.5, 3, 3.5],
  "Classique": [4, 3, 2, 1, -1, -1, 0, 2, 3, 3.5],
  "Électronique": [5, 4, 1.5, 0, -2, 1.5, 0.5, 1, 4, 5],
  "Voix": [-2, -3, -3, 1, 3.5, 3.5, 3, 1.5, 0, -2],
  "Loudness": [6, 4, 0, 0, -2, 0, -1, -5, 5, 1],
};
let eq = { enabled: false, preamp: 0, gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], preset: "Plat" };
const fmtDb = (v) => (v > 0 ? "+" : "") + v.toFixed(1).replace(".", DEC);

function sliderFill(input) {
  const v = Number(input.value), min = Number(input.min), max = Number(input.max);
  // Remplissage depuis 0 dB : du centre vers le haut ou vers le bas.
  const p = ((v - min) / (max - min)) * 100;
  input.style.setProperty("--p", (v >= 0 ? 50 : p) + "%");
  input.style.background =
    v >= 0
      ? `linear-gradient(to top, var(--track) 50%, var(--accent) 50%, var(--accent) ${p}%, var(--track) ${p}%)`
      : `linear-gradient(to top, var(--track) ${p}%, var(--accent-neg) ${p}%, var(--accent-neg) 50%, var(--track) 50%)`;
}

const allPresets = () => ({ ...EQ_PRESETS, ...Object.fromEntries(Object.entries(ui.eqPresets || {}).map(([k, v]) => [k, v.gains])) });
function renderEq() {
  $("eq-on").checked = eq.enabled;
  $("eq-btn").classList.toggle("on", eq.enabled);
  $("eq-per-device").checked = !!ui.eqPerDevice;
  $("eq-del").hidden = !(ui.eqPresets || {})[eq.preset];
  const names = Object.keys(allPresets());
  const presetOpts = [...names, ...(names.includes(eq.preset) ? [] : ["Personnalisé"])];
  $("eq-preset").innerHTML = presetOpts.map((n) => `<option ${n === eq.preset || (n === "Personnalisé" && !names.includes(eq.preset)) ? "selected" : ""}>${esc(n)}</option>`).join("");
  const band = (cls, label, value, idx) => `<div class="band ${cls}">
      <span class="val">${fmtDb(value)}</span>
      <input type="range" min="-12" max="12" step="0.5" value="${value}" data-band="${idx}" aria-label="${label}">
      <span class="f">${label}</span>
    </div>`;
  $("eq-bands").innerHTML = band("pre", "Préampli", eq.preamp, "pre") + eq.gains.map((g, i) => band("", EQ_FREQS[i], g, i)).join("");
  $("eq-bands").classList.toggle("off", !eq.enabled);
  document.querySelectorAll("#eq-bands input").forEach(sliderFill);
}

let eqTimer;
function saveEq(now = false) {
  clearTimeout(eqTimer);
  const send = () => invoke("set_eq", { eq });
  if (now) send();
  else eqTimer = setTimeout(send, 40);
}

$("eq-bands").addEventListener("input", (e) => {
  const inp = e.target;
  if (!inp.dataset.band) return;
  const v = Number(inp.value);
  if (inp.dataset.band === "pre") eq.preamp = v;
  else {
    eq.gains[Number(inp.dataset.band)] = v;
    eq.preset = "Personnalisé";
    const sel = $("eq-preset");
    if (![...sel.options].some((o) => o.value === "Personnalisé")) sel.add(new Option("Personnalisé"));
    sel.value = "Personnalisé";
  }
  inp.parentElement.querySelector(".val").textContent = fmtDb(v);
  sliderFill(inp);
  saveEq();
});
$("eq-on").addEventListener("change", (e) => {
  eq.enabled = e.target.checked;
  renderEq();
  saveEq(true);
});
$("eq-preset").addEventListener("change", (e) => {
  const g = allPresets()[e.target.value];
  if (!g) return;
  const custom = (ui.eqPresets || {})[e.target.value];
  eq = { ...eq, gains: [...g], preamp: custom ? custom.preamp : eq.preamp, preset: e.target.value, enabled: true };
  renderEq();
  saveEq(true);
});
$("eq-reset").addEventListener("click", () => {
  eq = { ...eq, gains: [...EQ_PRESETS.Plat], preamp: 0, preset: "Plat" };
  renderEq();
  saveEq(true);
});
function toggleEqPanel(force) {
  const p = $("eq-panel");
  p.hidden = force === undefined ? !p.hidden : !force;
}
$("eq-btn").addEventListener("click", (e) => {
  e.stopPropagation();
  toggleEqPanel();
});
document.addEventListener("click", (e) => {
  const p = $("eq-panel");
  if (!p.hidden && !p.contains(e.target) && !$("eq-btn").contains(e.target)) p.hidden = true;
});

// --- mode travail : vitesse sans changer la hauteur, boucle début / fin -------------------

let loopA = null, loopB = null; // secondes
let tempoPct = 0; // changement en %, 0 = vitesse d'origine
let localAt = 0; // dernier réglage fait ici : on laisse au moteur le temps de le prendre en compte
const fresh = () => Date.now() - localAt < 700;
const practiceOn = () => !!state?.practice;
const fmtPrecise = (t) => {
  const m = Math.floor(t / 60), r = t - m * 60;
  return `${m}:${r.toFixed(1).padStart(4, "0")}`;
};

function setPractice(on) {
  localAt = Date.now();
  invoke("set_practice", { on });
  // Retour visuel immédiat, l'état du moteur confirmera.
  $("practice").hidden = !on;
  $("practice-btn").classList.toggle("on", on);
}
function setTempo(pct) {
  localAt = Date.now();
  tempoPct = Math.max(-50, Math.min(50, Math.round(pct)));
  invoke("set_tempo", { tempo: tempoPct });
  paintTempo();
}
function stepTempo(d) {
  if (!practiceOn()) setPractice(true);
  setTempo(tempoPct + d);
}
function paintTempo() {
  const el = $("tempo");
  if (document.activeElement !== el) el.value = 100 + tempoPct;
  el.style.setProperty("--t", el.value - 50 + "%");
  const v = $("tempo-val");
  v.textContent = 100 + tempoPct + " %";
  v.classList.toggle("changed", tempoPct !== 0);
}
function sendLoop() {
  localAt = Date.now();
  if (loopA != null && loopB != null && loopB > loopA) invoke("set_loop", { start: loopA, end: loopB });
  else invoke("set_loop", { start: null, end: null });
  paintLoop();
}
function markLoop(which) {
  const s = state || {};
  if (!s.path || !s.duration) return;
  const p = s.position || 0;
  if (!practiceOn()) setPractice(true);
  if (which === "a") {
    loopA = p;
    if (loopB != null && loopB <= p + 0.2) loopB = null;
  } else {
    if (loopA == null) loopA = 0;
    if (p <= loopA + 0.2) return toast("La fin de la boucle doit être après son début.");
    loopB = p;
    // On repart du début de la boucle pour l'entendre tout de suite.
    invoke("seek", { position: loopA });
  }
  sendLoop();
}
function clearLoop() {
  loopA = loopB = null;
  sendLoop();
}
function paintLoop() {
  const d = state?.duration || 0;
  const band = $("loop-band");
  band.hidden = !d || loopA == null;
  if (d && loopA != null) {
    band.style.setProperty("--la", (loopA / d) * 100 + "%");
    band.style.setProperty("--lb", ((loopB ?? loopA) / d) * 100 + "%");
    band.classList.toggle("half", loopB == null);
  }
  const set = (id, v) => {
    const b = $(id);
    b.classList.toggle("set", v != null);
    b.querySelector("b").textContent = v != null ? fmtPrecise(v) : "—";
  };
  set("loop-a", loopA);
  set("loop-b", loopB);
  $("loop-clear").disabled = loopA == null;
}
function syncPractice(s, prev) {
  if (s.path !== prev?.path) {
    // Nouvelle piste : le moteur a déjà oublié la boucle.
    loopA = loopB = null;
  }
  if (!fresh()) {
    if (s.loopRange) [loopA, loopB] = s.loopRange;
    else if (loopB != null) loopA = loopB = null;
    $("practice").hidden = !s.practice;
    $("practice-btn").classList.toggle("on", !!s.practice);
    tempoPct = Math.round(s.tempo || 0);
  }
  const avail = !!s.tempoAvailable;
  $("tempo").disabled = !avail;
  document.querySelectorAll("[data-tempo-step]").forEach((b) => (b.disabled = !avail));
  $("tempo-val").disabled = !avail;
  $("tempo-note").hidden = avail;
  paintTempo();
  paintLoop();
  const k = (id) => shortcuts[id] ? comboLabel(shortcuts[id]) : "";
  const keys = $("loop-keys");
  const html = shortcuts.loopStart || shortcuts.loopEnd ? `${k("loopStart")} début · ${k("loopEnd")} fin` : "";
  if (keys.innerHTML !== html) keys.innerHTML = html;
}

$("practice-btn").addEventListener("click", () => setPractice(!practiceOn()));
$("tempo").addEventListener("input", (e) => {
  $("tempo").style.setProperty("--t", e.target.value - 50 + "%");
  $("tempo-val").textContent = e.target.value + " %";
});
$("tempo").addEventListener("change", (e) => setTempo(+e.target.value - 100));
$("tempo").addEventListener("dblclick", () => setTempo(0));
$("tempo-val").addEventListener("click", () => setTempo(0));
document.querySelectorAll("[data-tempo-step]").forEach((b) => b.addEventListener("click", () => stepTempo(+b.dataset.tempoStep)));
$("loop-a").addEventListener("click", () => markLoop("a"));
$("loop-b").addEventListener("click", () => markLoop("b"));
$("loop-clear").addEventListener("click", clearLoop);

// Enregistrer le réglage actuel sous un nom.
$("eq-save").addEventListener("click", () => {
  const row = $("eq-save-row");
  row.hidden = !row.hidden;
  if (!row.hidden) {
    $("eq-name").value = (ui.eqPresets || {})[eq.preset] ? eq.preset : "";
    $("eq-name").focus();
  }
});
function saveEqPreset() {
  const name = $("eq-name").value.trim();
  if (!name) return;
  if (EQ_PRESETS[name]) return toast("Ce nom est déjà celui d'un préréglage fourni.");
  ui.eqPresets = { ...(ui.eqPresets || {}), [name]: { gains: [...eq.gains], preamp: eq.preamp } };
  eq.preset = name;
  saveUi();
  saveEq(true);
  $("eq-save-row").hidden = true;
  renderEq();
  toast(`Préréglage « ${name} » enregistré`);
}
$("eq-name-ok").addEventListener("click", saveEqPreset);
$("eq-name").addEventListener("keydown", (e) => {
  if (e.key === "Enter") saveEqPreset();
  if (e.key === "Escape") $("eq-save-row").hidden = true;
  e.stopPropagation();
});
$("eq-del").addEventListener("click", () => {
  if (!(ui.eqPresets || {})[eq.preset]) return;
  const { [eq.preset]: _, ...rest } = ui.eqPresets;
  ui.eqPresets = rest;
  toast(`Préréglage « ${eq.preset} » supprimé`);
  eq.preset = "Personnalisé";
  saveUi();
  saveEq(true);
  renderEq();
});

// Un réglage par sortie (casque, enceintes…) : on retient l'égaliseur de chaque périphérique.
let eqDevice = null;
$("eq-per-device").addEventListener("change", (e) => {
  ui.eqPerDevice = e.target.checked;
  if (ui.eqPerDevice && eqDevice) ui.eqByDevice = { ...(ui.eqByDevice || {}), [eqDevice]: { ...eq } };
  saveUi();
  if (ui.eqPerDevice) toast(eqDevice ? `Réglage retenu pour « ${eqDevice} »` : "Réglage retenu pour chaque sortie");
});
function eqFollowDevice(dev) {
  if (!dev || dev === eqDevice) return;
  const prevDev = eqDevice;
  eqDevice = dev;
  if (!ui.eqPerDevice) return;
  const byDev = { ...(ui.eqByDevice || {}) };
  if (prevDev) byDev[prevDev] = { ...eq, gains: [...eq.gains] };
  ui.eqByDevice = byDev;
  const saved = byDev[dev];
  if (saved) {
    eq = { ...eq, ...saved, gains: [...saved.gains] };
    renderEq();
    saveEq(true);
    toast(`Égaliseur de « ${dev} »`);
  }
  saveUi();
}
// Le réglage en cours suit aussi le périphérique actif.
setInterval(() => {
  if (ui.eqPerDevice && eqDevice) {
    const cur = (ui.eqByDevice || {})[eqDevice];
    if (!cur || JSON.stringify(cur) !== JSON.stringify(eq)) {
      ui.eqByDevice = { ...(ui.eqByDevice || {}), [eqDevice]: { ...eq, gains: [...eq.gains] } };
      saveUi();
    }
  }
}, 5000);

// Import AutoEQ (fichiers « ParametricEQ.txt » ou « GraphicEQ.txt » d'autoeq.app).
$("eq-import").addEventListener("click", () => $("eq-file").click());
$("eq-file").addEventListener("change", async (e) => {
  const f = e.target.files[0];
  e.target.value = "";
  if (!f) return;
  const r = parseAutoEq(await f.text());
  if (!r) return toast("Fichier non reconnu : il faut un ParametricEQ.txt ou un GraphicEQ.txt d'AutoEQ.", "error");
  const name = "AutoEQ · " + f.name.replace(/\.txt$/i, "").replace(/ ?(Parametric|Graphic)EQ$/i, "");
  eq = { ...eq, enabled: true, preamp: r.preamp, gains: r.gains, preset: name };
  ui.eqPresets = { ...(ui.eqPresets || {}), [name]: { gains: [...r.gains], preamp: r.preamp } };
  saveUi();
  renderEq();
  saveEq(true);
  toast(`Correction importée : ${name}`);
});
const EQ_HZ = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
/** Réponse d'un filtre RBJ (dB) à la fréquence f. */
function biquadDb(type, fc, gain, q, f, fs = 48000) {
  const A = 10 ** (gain / 40), w0 = (2 * Math.PI * fc) / fs, alpha = Math.sin(w0) / (2 * q), cs = Math.cos(w0);
  let b0, b1, b2, a0, a1, a2;
  if (type === "PK") {
    [b0, b1, b2, a0, a1, a2] = [1 + alpha * A, -2 * cs, 1 - alpha * A, 1 + alpha / A, -2 * cs, 1 - alpha / A];
  } else {
    const sq = 2 * Math.sqrt(A) * alpha;
    if (type === "LSC") {
      [b0, b1, b2] = [A * (A + 1 - (A - 1) * cs + sq), 2 * A * (A - 1 - (A + 1) * cs), A * (A + 1 - (A - 1) * cs - sq)];
      [a0, a1, a2] = [A + 1 + (A - 1) * cs + sq, -2 * (A - 1 + (A + 1) * cs), A + 1 + (A - 1) * cs - sq];
    } else {
      [b0, b1, b2] = [A * (A + 1 + (A - 1) * cs + sq), -2 * A * (A - 1 + (A + 1) * cs), A * (A + 1 + (A - 1) * cs - sq)];
      [a0, a1, a2] = [A + 1 - (A - 1) * cs + sq, 2 * (A - 1 - (A + 1) * cs), A + 1 - (A - 1) * cs - sq];
    }
  }
  const w = (2 * Math.PI * f) / fs;
  const re = (c0, c1, c2) => c0 + c1 * Math.cos(w) + c2 * Math.cos(2 * w);
  const im = (c1, c2) => -(c1 * Math.sin(w) + c2 * Math.sin(2 * w));
  const num = Math.hypot(re(b0, b1, b2), im(b1, b2)), den = Math.hypot(re(a0, a1, a2), im(a1, a2));
  return 20 * Math.log10(num / den);
}
function parseAutoEq(txt) {
  const clamp = (v) => Math.max(-12, Math.min(12, Math.round(v * 2) / 2));
  const pre = /Preamp:\s*(-?[\d.]+)/i.exec(txt);
  const filters = [...txt.matchAll(/Filter\s*\d*:\s*ON\s+(PK|LSC|HSC|LS|HS)\s+Fc\s+([\d.]+)\s*Hz\s+Gain\s+(-?[\d.]+)\s*dB(?:\s+Q\s+([\d.]+))?/gi)];
  if (filters.length) {
    const gains = EQ_HZ.map((f) =>
      clamp(filters.reduce((s, m) => s + biquadDb({ LS: "LSC", HS: "HSC" }[m[1].toUpperCase()] || m[1].toUpperCase(), +m[2], +m[3], +(m[4] || 0.707), f), 0))
    );
    return { gains, preamp: clamp(pre ? +pre[1] : -Math.max(0, ...gains)) };
  }
  const g = /GraphicEQ:\s*(.+)/i.exec(txt);
  if (g) {
    const pts = g[1].split(";").map((x) => x.trim().split(/\s+/).map(Number)).filter((p) => p.length === 2 && !p.some(isNaN));
    if (!pts.length) return null;
    const at = (f) => {
      if (f <= pts[0][0]) return pts[0][1];
      for (let i = 1; i < pts.length; i++) {
        if (f <= pts[i][0]) {
          const [f0, g0] = pts[i - 1], [f1, g1] = pts[i];
          const t = Math.log(f / f0) / Math.log(f1 / f0);
          return g0 + t * (g1 - g0);
        }
      }
      return pts[pts.length - 1][1];
    };
    const gains = EQ_HZ.map((f) => clamp(at(f)));
    return { gains, preamp: clamp(-Math.max(0, ...gains)) };
  }
  return null;
}

// --- apparence : clair / sombre / système -------------------------------------------------

const MODES = [
  ["light", "Clair", "#EEF1F5", "#171C28"],
  ["dark", "Sombre", "#121212", "#EDEDED"],
  ["system", "Comme Windows", "linear-gradient(135deg, #EEF1F5 50%, #121212 50%)", "#8A90A0"],
];
let mode = "light";
const systemDark = window.matchMedia("(prefers-color-scheme: dark)");
function applyMode(m) {
  mode = MODES.some(([id]) => id === m) ? m : "light";
  const dark = mode === "dark" || (mode === "system" && systemDark.matches);
  if (dark) document.documentElement.dataset.mode = "dark";
  else delete document.documentElement.dataset.mode;
  try {
    localStorage.setItem("mode", mode);
  } catch {}
  applyAccent();
}
systemDark.addEventListener("change", () => mode === "system" && applyMode(mode));

// --- couleur d'accent : corail, ou celle choisie dans Windows ----------------------------

let accentPref = "coral";
let winAccent = null; // { accent: "#rrggbb", palette: [8 nuances, de la plus claire à la plus foncée] }
const hexRgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const luminance = ([r, g, b]) => {
  const c = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
};
/** Nuance Windows adaptée au thème : plus claire en sombre, plus soutenue en clair (comme Windows). */
function windowsShade() {
  if (!winAccent?.accent) return null;
  const dark = document.documentElement.dataset.mode === "dark";
  const p = winAccent.palette || [];
  return (dark ? p[1] : p[4]) || winAccent.accent;
}
function applyAccent() {
  const st = document.documentElement.style;
  const hex = accentPref === "windows" ? windowsShade() : null;
  if (!hex) {
    st.removeProperty("--accent-rgb");
    st.removeProperty("--on-accent");
    return;
  }
  const rgb = hexRgb(hex);
  st.setProperty("--accent-rgb", rgb.join(" "));
  // Texte sur fond d'accent : blanc, sauf si l'accent est très clair.
  st.setProperty("--on-accent", luminance(rgb) > 0.45 ? "#111111" : "#FFFFFF");
}
async function refreshWindowsAccent() {
  try {
    winAccent = await invoke("windows_accent");
  } catch {
    winAccent = null;
  }
  applyAccent();
  const sw = document.querySelector('[data-accent-pick="windows"] .swatch');
  if (sw && winAccent?.accent) sw.style.background = winAccent.accent;
}
// Couleur changée dans Windows : on la reprend au retour dans l'app.
window.addEventListener("focus", () => accentPref === "windows" && refreshWindowsAccent());
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-accent-pick]");
  if (!b) return;
  accentPref = b.dataset.accentPick;
  invoke("set_accent", { accent: accentPref });
  document.querySelectorAll("[data-accent-pick]").forEach((c) => c.classList.toggle("active", c.dataset.accentPick === accentPref));
  refreshWindowsAccent();
});

// --- raccourcis clavier ----------------------------------------------------------------

const ACTIONS = [
  ["toggle", "Lecture / pause"],
  ["next", "Piste suivante"],
  ["prev", "Piste précédente"],
  ["forward", "Avancer de 10 s"],
  ["backward", "Reculer de 10 s"],
  ["volUp", "Monter le volume"],
  ["volDown", "Baisser le volume"],
  ["mute", "Couper / rétablir le son"],
  ["eq", "Afficher l'égaliseur"],
  ["eqToggle", "Activer / désactiver l'égaliseur"],
  ["search", "Rechercher"],
  ["nowPlaying", "Aller à la piste en cours"],
  ["back", "Retour"],
  ["tabMusic", "Onglet Musique"],
  ["tabAlbums", "Onglet Albums"],
  ["tabFolders", "Onglet Dossiers"],
  ["tabPlaylists", "Onglet Playlists"],
  ["like", "J'aime le titre en cours"],
  ["ambient", "Mode ambiance (plein écran)"],
  ["settings", "Paramètres"],
  ["practice", "Mode travail (afficher / masquer)"],
  ["loopStart", "Boucle : début ici"],
  ["loopEnd", "Boucle : fin ici"],
  ["loopClear", "Boucle : effacer"],
  ["slower", "Ralentir (−5 %)"],
  ["faster", "Accélérer (+5 %)"],
];
const DEFAULT_SHORTCUTS = {
  toggle: "Space",
  next: "Ctrl+ArrowRight",
  prev: "Ctrl+ArrowLeft",
  forward: "ArrowRight",
  backward: "ArrowLeft",
  volUp: "Ctrl+ArrowUp",
  volDown: "Ctrl+ArrowDown",
  mute: "Ctrl+M",
  eq: "Ctrl+E",
  eqToggle: "Ctrl+Shift+E",
  search: "Ctrl+F",
  nowPlaying: "Ctrl+J",
  back: "Alt+ArrowLeft",
  tabMusic: "Ctrl+1",
  tabAlbums: "Ctrl+2",
  tabFolders: "Ctrl+3",
  tabPlaylists: "Ctrl+4",
  like: "Ctrl+L",
  ambient: "F11",
  settings: "Ctrl+,",
  practice: "Ctrl+P",
  loopStart: "F7",
  loopEnd: "F8",
  loopClear: "F9",
  slower: "Ctrl+Shift+ArrowDown",
  faster: "Ctrl+Shift+ArrowUp",
};

let shortcuts = { ...DEFAULT_SHORTCUTS };
let captureFor = null;
let lastVolume = 1;

function comboOf(e) {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  let k = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift"].filter(Boolean);
  return [...mods, k].join("+");
}
const KEY_LABELS = { Ctrl: "Ctrl", Shift: "Maj", Alt: "Alt", Space: "Espace", ArrowRight: "→", ArrowLeft: "←", ArrowUp: "↑", ArrowDown: "↓", Enter: "Entrée", Escape: "Échap", Backspace: "Retour arrière", Delete: "Suppr", PageUp: "Page ↑", PageDown: "Page ↓", Home: "Début", End: "Fin" };
if (LANG === "en") Object.assign(KEY_LABELS, { Shift: "Shift", Space: "Space", Enter: "Enter", Escape: "Esc", Backspace: "Backspace", Delete: "Del", Home: "Home", End: "End" });
const comboLabel = (c) => (c ? c.split("+").map((k) => `<kbd>${esc(KEY_LABELS[k] || k)}</kbd>`).join("") : `<span class="hint">Aucun</span>`);

function shortcutsSection() {
  return `<section class="section">
    <div class="section-head"><h2>Raccourcis clavier</h2><button class="btn small" data-shortcuts-reset>Tout réinitialiser</button></div>
    <p class="hint" style="margin:0">Clique sur un raccourci puis appuie sur la nouvelle combinaison. Échap annule, Retour arrière efface. Les touches multimédia du clavier (lecture, suivant, précédent) fonctionnent partout, même quand l'app est en arrière-plan.</p>
    <div id="shortcut-rows" class="shortcuts">${shortcutRows()}</div>
    <p id="shortcut-note" class="hint" style="margin:0"></p>
  </section>`;
}
function shortcutRows() {
  return ACTIONS.map(
    ([id, label]) => `<div class="sc-row">
      <span>${esc(label)}</span>
      <button class="sc-key ${captureFor === id ? "capturing" : ""}" data-shortcut="${id}">${captureFor === id ? "Appuie sur une touche…" : comboLabel(shortcuts[id])}</button>
      <button class="sc-clear" data-shortcut-clear="${id}" title="Effacer" aria-label="Effacer">${icon("x")}</button>
    </div>`
  ).join("");
}
function renderShortcutRows() {
  const el = $("shortcut-rows");
  if (el) el.innerHTML = shortcutRows();
}
function saveShortcuts(note = "") {
  invoke("set_shortcuts", { shortcuts });
  renderShortcutRows();
  const n = $("shortcut-note");
  if (n) n.textContent = note;
}

function runAction(a) {
  const s = state || {};
  switch (a) {
    case "toggle": return invoke("toggle");
    case "next": return invoke("next");
    case "prev": return invoke("prev");
    case "forward": return s.duration && invoke("seek", { position: Math.min(s.duration - 0.5, (s.position || 0) + 10) });
    case "backward": return s.duration && invoke("seek", { position: Math.max(0, (s.position || 0) - 10) });
    case "volUp":
    case "volDown": {
      const v = Math.max(0, Math.min(1, (s.volume ?? 1) + (a === "volUp" ? 0.05 : -0.05)));
      $("volume").value = Math.round(v * 100);
      return invoke("set_volume", { volume: v });
    }
    case "mute": {
      const cur = s.volume ?? 1;
      if (cur > 0) lastVolume = cur;
      const v = cur > 0 ? 0 : lastVolume || 1;
      $("volume").value = Math.round(v * 100);
      return invoke("set_volume", { volume: v });
    }
    case "eq": return toggleEqPanel();
    case "eqToggle":
      eq.enabled = !eq.enabled;
      renderEq();
      return saveEq(true);
    case "search": $("search").focus(); return $("search").select();
    case "nowPlaying": {
      const t = trackByPath.get(s.path);
      return t && go({ view: "music", artist: artistOf(t), album: albumKeyOf(t) });
    }
    case "back": return $("back").click();
    case "tabMusic": return document.querySelector('.tabs [data-view="music"]').click();
    case "tabAlbums": return document.querySelector('.tabs [data-view="albums"]').click();
    case "tabFolders": return document.querySelector('.tabs [data-view="folders"]').click();
    case "tabPlaylists": return document.querySelector('.tabs [data-view="playlists"]').click();
    case "ambient": return ambientOpen ? closeAmbient() : openAmbient();
    case "like": return s.path && collOp({ op: "likeTrack", path: s.path, on: !likedT.has(s.path) });
    case "settings": return document.querySelector('.icon-btn[data-view="settings"]').click();
    case "practice": return setPractice(!practiceOn());
    case "loopStart": return markLoop("a");
    case "loopEnd": return markLoop("b");
    case "loopClear": return clearLoop();
    case "slower": return stepTempo(-5);
    case "faster": return stepTempo(5);
  }
}

document.addEventListener("keydown", (e) => {
  // Saisie d'un nouveau raccourci dans les paramètres.
  if (captureFor) {
    e.preventDefault();
    if (e.key === "Escape") {
      captureFor = null;
      return renderShortcutRows();
    }
    if (e.key === "Backspace" || e.key === "Delete") {
      shortcuts[captureFor] = "";
      captureFor = null;
      return saveShortcuts();
    }
    const c = comboOf(e);
    if (!c) return;
    let note = "";
    for (const [id, label] of ACTIONS) {
      if (id !== captureFor && shortcuts[id] === c) {
        shortcuts[id] = "";
        note = `Ce raccourci était utilisé par « ${label} », il lui a été retiré.`;
      }
    }
    shortcuts[captureFor] = c;
    captureFor = null;
    return saveShortcuts(note);
  }


  const t = e.target;
  const typing = (t.tagName === "INPUT" && t.type !== "range" && t.type !== "checkbox" && t.type !== "radio") || t.tagName === "SELECT" || t.tagName === "TEXTAREA";
  if (e.key === "Escape" && ambientOpen) return closeAmbient();
  if (e.key === "Escape") {
    if (typing) t.blur();
    toggleEqPanel(false);
    $("out-panel").hidden = true;
    return;
  }
  const c = comboOf(e);
  if (!c) return;
  // En train d'écrire : seuls les raccourcis avec Ctrl ou Alt restent actifs.
  if (typing && !e.ctrlKey && !e.altKey) return;
  const plainChar = !e.ctrlKey && !e.altKey && !e.metaKey && e.key.length === 1 && /[\p{L}\p{N}]/u.test(e.key);
  const inList = !!t.closest?.("#artist-list");
  // Dans la liste des artistes, une lettre saute à l'artiste suivant qui commence par elle.
  if (plainChar && inList && jumpToLetter(e.key)) return e.preventDefault();
  const action = Object.keys(shortcuts).find((k) => shortcuts[k] === c);
  if (action) {
    e.preventDefault();
    return runAction(action);
  }
  if (plainChar && route.view === "music" && jumpToLetter(e.key)) e.preventDefault();
});

// Saut par lettre : premier artiste qui commence par la lettre, puis le suivant à chaque nouvel appui.
function jumpToLetter(key) {
  const list = $("artist-list");
  if (!list) return false;
  const letter = firstLetter(key);
  const rows = [...list.querySelectorAll(".artist-row")];
  const matches = rows.filter((r) => r.dataset.letterOf === letter);
  if (!matches.length) return true;
  const cur = document.activeElement?.closest?.(".artist-row");
  const from = cur && cur.dataset.letterOf === letter ? matches.indexOf(cur) : -1;
  const target = matches[(from + 1) % matches.length];
  target.focus({ preventScroll: true });
  scrollToChild(list, target, "nearest");
  return true;
}

// --- lecteur flottant : réserve sa hauteur en bas des panneaux -------------------------

new ResizeObserver(([e]) => {
  document.documentElement.style.setProperty("--player-h", Math.round(e.target.getBoundingClientRect().height) + "px");
}).observe(document.querySelector(".player"));

// --- mode ambiance : plein écran, pochette, couleurs tirées de la pochette -----------------

let ambientOpen = false;
let ambientWasMax = false;
let ambientPath = null;
let ambientLayer = 0;
let ambientTimer = null;
let ambientSettings = { auto: false, delay: 300 };
const amb = $("ambient");

async function openAmbient() {
  if (ambientOpen) return;
  if (!state?.path) return toast("Lance un morceau pour passer en mode ambiance.");
  ambientOpen = true;
  ambientPath = null;
  amb.dataset.style = ui.ambientStyle || "cover";
  amb.classList.toggle("no-clock", !ui.ambientClock);
  amb.hidden = false;
  requestAnimationFrame(() => amb.classList.add("show"));
  ambientUpdate(state, null);
  wake();
  clockTick();
  // Fenêtre agrandie : Windows garde la place de la barre des tâches en plein écran
  // (bande noire en bas). On la « désagrandit » d'abord, et on la remet en sortant.
  try {
    const win = window.__TAURI__.window.getCurrentWindow();
    ambientWasMax = await win.isMaximized();
    if (ambientWasMax) await win.unmaximize();
    await win.setFullscreen(true);
  } catch {}
  invoke("keep_awake", { on: true });
}
async function closeAmbient() {
  if (!ambientOpen) return;
  ambientOpen = false;
  amb.classList.remove("show", "awake");
  setTimeout(() => !ambientOpen && (amb.hidden = true), 400);
  try {
    const win = window.__TAURI__.window.getCurrentWindow();
    await win.setFullscreen(false);
    if (ambientWasMax) await win.maximize();
  } catch {}
  invoke("keep_awake", { on: false });
}

/** Couleurs marquantes d'une pochette : jusqu'à 3, vives et bien distinctes. */
function paletteOf(img) {
  const c = document.createElement("canvas");
  c.width = c.height = 40;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  ctx.drawImage(img, 0, 0, 40, 40);
  const d = ctx.getImageData(0, 0, 40, 40).data;
  const buckets = new Map();
  for (let i = 0; i < d.length; i += 4) {
    const k = ((d[i] >> 4) << 8) | ((d[i + 1] >> 4) << 4) | (d[i + 2] >> 4);
    const b = buckets.get(k) || [0, 0, 0, 0];
    b[0] += d[i]; b[1] += d[i + 1]; b[2] += d[i + 2]; b[3]++;
    buckets.set(k, b);
  }
  const hsl = ([r, g, b]) => {
    r /= 255; g /= 255; b /= 255;
    const mx = Math.max(r, g, b), mn = Math.min(r, g, b), l = (mx + mn) / 2;
    const sat = mx === mn ? 0 : (mx - mn) / (1 - Math.abs(2 * l - 1));
    return [sat, l];
  };
  const cands = [...buckets.values()]
    .map((b) => {
      const rgb = [b[0] / b[3], b[1] / b[3], b[2] / b[3]];
      const [sat, l] = hsl(rgb);
      const lw = l < 0.12 || l > 0.9 ? 0.15 : 1;
      return { rgb, score: b[3] * (0.25 + sat) * lw };
    })
    .sort((a, b) => b.score - a.score);
  const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
  const out = [];
  for (const cnd of cands) {
    if (out.every((o) => dist(o, cnd.rgb) > 70)) out.push(cnd.rgb);
    if (out.length === 3) break;
  }
  while (out.length < 3) out.push(out[0] || [60, 60, 64]);
  return out;
}
/** Couleur lisible sur fond sombre (pour la barre de progression). */
function brighten([r, g, b]) {
  const mx = Math.max(r, g, b, 1);
  const k = mx < 200 ? 200 / mx : 1;
  return [r, g, b].map((v) => Math.min(255, Math.round(v * k + 20)));
}
const rgbStr = (c, a = 1) => `rgb(${c.map(Math.round).join(" ")} / ${a})`;

function ambientColors(path) {
  const im = new Image();
  im.crossOrigin = "anonymous";
  im.onload = () => {
    if (path !== ambientPath) return;
    let p;
    try {
      p = paletteOf(im);
    } catch {
      return;
    }
    const st = amb.style;
    st.setProperty("--amb-c1", rgbStr(p[0], 0.62));
    st.setProperty("--amb-c2", rgbStr(p[1], 0.55));
    st.setProperty("--amb-c3", rgbStr(p[2], 0.5));
    st.setProperty("--amb-accent", rgbStr(brighten(p[0])));
  };
  im.src = coverUrl(path);
}

function ambientUpdate(s, prev) {
  if (!s?.path) return;
  if (s.path !== ambientPath) {
    ambientPath = s.path;
    const t = trackByPath.get(s.path);
    const url = coverUrl(s.path);
    // Fond flou : fondu enchaîné entre deux calques.
    const layers = amb.querySelectorAll(".amb-layer");
    ambientLayer = 1 - ambientLayer;
    const next = layers[ambientLayer], old = layers[1 - ambientLayer];
    next.onload = () => {
      next.classList.add("on");
      old.classList.remove("on");
    };
    next.onerror = () => {
      next.classList.remove("on");
      old.classList.remove("on");
    };
    next.src = url;
    const cov = $("amb-cover");
    cov.classList.remove("in");
    void cov.offsetWidth;
    cov.onerror = () => (cov.src = BLANK);
    cov.src = url;
    cov.classList.add("in");
    $("amb-title").textContent = t ? t.title : s.path.split(/[\\/]/).pop();
    $("amb-artist").textContent = t ? t.artist : "";
    $("amb-album").textContent = t ? [t.album, t.year].filter(Boolean).join(" · ") : "";
    const info = amb.querySelector(".amb-info");
    info.classList.remove("in");
    void info.offsetWidth;
    info.classList.add("in");
    ambientColors(s.path);
    const like = $("amb-like");
    like.dataset.likeTrack = s.path;
    like.classList.toggle("on", likedT.has(s.path));
    // À suivre.
    const qi = s.index ?? -1;
    const nx = queue[qi]?.path === s.path ? queue[qi + 1] : null;
    $("amb-next").innerHTML = nx ? `<span>À suivre</span> ${esc(nx.title)} <span>· ${esc(nx.artist)}</span>` : "";
  }
  const p = s.duration ? Math.min(1, s.position / s.duration) : 0;
  $("amb-fill").style.transform = `scaleX(${p})`;
  $("amb-pos").textContent = fmtTime(s.position);
  $("amb-dur").textContent = fmtTime(s.duration);
  $("amb-play-ic").setAttribute("href", s.playing ? "#i-pause" : "#i-play");
  amb.classList.toggle("paused", !s.playing);
  if (document.activeElement !== $("amb-volume") && Date.now() - ambVolAt > 800) paintAmbVolume(Math.round((s.volume ?? 1) * 100));
}

// Volume du mode ambiance : curseur, et molette de la souris n'importe où sur l'écran.
let ambVolAt = 0;
function paintAmbVolume(v) {
  const el = $("amb-volume");
  el.value = v;
  el.style.setProperty("--v", v + "%");
  $("amb-vol-val").textContent = v;
  $("amb-vol-ic").setAttribute("href", v === 0 ? "#i-volume-x" : v < 50 ? "#i-volume-1" : "#i-volume-2");
}
function setAmbVolume(v) {
  v = Math.max(0, Math.min(100, Math.round(v)));
  ambVolAt = Date.now();
  paintAmbVolume(v);
  $("volume").value = v;
  paintVolume();
  invoke("set_volume", { volume: v / 100 });
}
$("amb-volume").addEventListener("input", (e) => setAmbVolume(+e.target.value));
amb.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    wake();
    setAmbVolume(+$("amb-volume").value + (e.deltaY < 0 ? 5 : -5));
  },
  { passive: false }
);

function clockTick() {
  if (!ambientOpen) return;
  const d = new Date();
  $("amb-clock").textContent = d.toLocaleTimeString(LOCALE, { hour: "2-digit", minute: "2-digit" });
  setTimeout(clockTick, (60 - d.getSeconds()) * 1000 + 50);
}

// Les commandes et la souris apparaissent au moindre mouvement, puis s'effacent.
function wake() {
  amb.classList.add("awake");
  clearTimeout(ambientTimer);
  ambientTimer = setTimeout(() => amb.classList.remove("awake"), 3000);
}
amb.addEventListener("mousemove", wake);
amb.addEventListener("click", (e) => {
  wake();
  const b = e.target.closest("[data-amb]");
  if (b) invoke(b.dataset.amb);
});
$("amb-close").addEventListener("click", closeAmbient);
$("ambient-btn").addEventListener("click", openAmbient);

// Lancement automatique quand personne n'a touché l'ordinateur depuis un moment.
setInterval(async () => {
  if (!ambientSettings.auto || ambientOpen || !state?.playing) return;
  try {
    const idle = await invoke("idle_seconds");
    if (idle >= ambientSettings.delay) openAmbient();
  } catch {}
}, 15000);


document.addEventListener("change", (e) => {
  if (e.target.id !== "amb-auto" && e.target.id !== "amb-delay") return;
  ambientSettings = { auto: $("amb-auto").checked, delay: +$("amb-delay").value };
  invoke("set_ambient", ambientSettings);
});

// --- fenêtre sans barre Windows : boutons réduire / agrandir / fermer ------------------

{
  const win = window.__TAURI__.window.getCurrentWindow();
  $("win-min").addEventListener("click", () => win.minimize());
  $("win-max").addEventListener("click", () => win.toggleMaximize());
  $("win-close").addEventListener("click", () => win.close());
  const syncMax = async () => {
    const max = await win.isMaximized();
    document.body.classList.toggle("maximized", max);
    $("win-max").querySelector("use").setAttribute("href", max ? "#i-copy" : "#i-square");
    $("win-max").title = max ? "Restaurer" : "Agrandir";
  };
  win.onResized(syncMax);
  syncMax();
}

// =========================================================================================
// Gestion de la bibliothèque : sélection, menu contextuel, éditeur, historique
// =========================================================================================

// --- sélection de pistes (clic, Ctrl+clic, Maj+clic) -------------------------------------

const selected = new Set();
let anchor = null; // { list, i }

function paintSelection() {
  document.querySelectorAll("[data-path][data-i]").forEach((r) => r.classList.toggle("sel", selected.has(r.dataset.path)));
}

function selectRow(el, e) {
  const list = Number(el.dataset.list), i = Number(el.dataset.i), path = el.dataset.path;
  if (e.shiftKey && anchor && anchor.list === list) {
    if (!e.ctrlKey) selected.clear();
    const [a, b] = [Math.min(anchor.i, i), Math.max(anchor.i, i)];
    for (let k = a; k <= b; k++) selected.add(lists[list][k].path);
  } else if (e.ctrlKey) {
    selected.has(path) ? selected.delete(path) : selected.add(path);
    anchor = { list, i };
  } else {
    selected.clear();
    selected.add(path);
    anchor = { list, i };
  }
  paintSelection();
}

const selectedTracks = () => [...selected].map((p) => trackByPath.get(p)).filter(Boolean);

// --- petit message temporaire -------------------------------------------------------------

function toast(msg, kind = "") {
  let el = $("toast");
  if (!el) {
    el = document.createElement("div");
    el.id = "toast";
    document.body.append(el);
  }
  el.className = "toast show " + kind;
  el.textContent = msg;
  clearTimeout(el._t);
  el._t = setTimeout(() => el.classList.remove("show"), kind === "error" ? 7000 : 3500);
}

// --- menu contextuel ------------------------------------------------------------------------

function showMenu(x, y, items) {
  closeMenu();
  const m = document.createElement("div");
  m.id = "ctx";
  m.className = "ctx";
  m.innerHTML = items
    .map((it, k) => (it === "-" ? `<hr>` : `<button data-k="${k}" ${it.disabled ? "disabled" : ""}>${it.icon ? icon(it.icon) : ""}<span>${esc(it.label)}</span></button>`))
    .join("");
  document.body.append(m);
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(x, innerWidth - r.width - 8) + "px";
  m.style.top = Math.min(y, innerHeight - r.height - 8) + "px";
  m.addEventListener("click", (e) => {
    const b = e.target.closest("button[data-k]");
    if (!b) return;
    closeMenu();
    items[Number(b.dataset.k)].run();
  });
}
function closeMenu() {
  $("ctx")?.remove();
}
document.addEventListener("click", (e) => !e.target.closest("#ctx") && closeMenu());
document.addEventListener("scroll", closeMenu, true);

function albumMenuItems(a) {
  return [
    { label: "Lire", icon: "play", run: () => playList(a.tracks) },
    "-",
    { label: "Modifier les infos…", icon: "pencil", run: () => openEditor(a.tracks, { tab: "infos", title: a.title }) },
    { label: "Changer la pochette…", icon: "image", run: () => openEditor(a.tracks, { tab: "cover", title: a.title }) },
    { label: "Corriger avec MusicBrainz / Discogs…", icon: "wand", run: () => openEditor(a.tracks, { tab: "fix", title: a.title }) },
    { label: "Renommer les fichiers…", icon: "folder", run: () => openEditor(a.tracks, { tab: "files", title: a.title }) },
  ];
}

$("view").addEventListener("contextmenu", (e) => {
  // Fichier d'une playlist qui n'existe plus : on peut seulement le retirer.
  const missing = e.target.closest("tr.missing");
  if (missing && route.view === "playlists") {
    e.preventDefault();
    const pl = currentPlaylist();
    const path = missing.dataset.path;
    return showMenu(e.clientX, e.clientY, [
      { label: pl ? "Retirer de la playlist" : "Retirer des titres aimés", icon: "trash", run: () => (pl ? collOp({ op: "remove", id: pl.id, paths: [path] }) : likeTracks([path], false)) },
    ]);
  }
  const row = e.target.closest("[data-path][data-i]");
  const block = e.target.closest("[data-block]");
  const card = e.target.closest("[data-open-album]");
  const artistRow = e.target.closest(".artist-row, .card.artist");
  let items = null;

  if (row) {
    if (!selected.has(row.dataset.path)) selectRow(row, { ctrlKey: false, shiftKey: false });
    const ts = selectedTracks();
    const n = ts.length;
    const paths = ts.map((t) => t.path);
    const allLiked = paths.every((p) => likedT.has(p));
    const pl = route.view === "playlists" ? currentPlaylist() : null;
    const x = e.clientX, y = e.clientY;
    items = [
      { label: n > 1 ? `Lire les ${n} titres` : "Lire", icon: "play", run: () => playList(ts) },
      "-",
      { label: allLiked ? "Je n'aime plus" : "J'aime", icon: "heart", run: () => likeTracks(paths, !allLiked) },
      { label: "Ajouter à une playlist…", icon: "list-music", run: () => setTimeout(() => playlistPicker(x, y, paths)) },
      ...(pl ? [{ label: "Retirer de la playlist", icon: "trash", run: () => collOp({ op: "remove", id: pl.id, paths }) }] : []),
      ...(route.view === "playlists" && route.pl === "liked" ? [{ label: "Retirer des titres aimés", icon: "trash", run: () => likeTracks(paths, false) }] : []),
      "-",
      { label: n > 1 ? `Modifier ${n} titres…` : "Modifier le titre…", icon: "pencil", run: () => openEditor(ts, { tab: "infos" }) },
      { label: "Changer la pochette…", icon: "image", run: () => openEditor(ts, { tab: "cover" }) },
      { label: "Renommer les fichiers…", icon: "folder", run: () => openEditor(ts, { tab: "files" }) },
    ];
  } else if (block || card) {
    const a = albumByKey.get(block ? block.dataset.block : card.dataset.openAlbum);
    if (a) {
      const x = e.clientX, y = e.clientY;
      const k = likeKeyOf(a);
      items = albumMenuItems(a);
      items.splice(1, 0,
        { label: likedA.has(k) ? "Je n'aime plus l'album" : "J'aime l'album", icon: "heart", run: () => collOp({ op: "likeAlbum", key: k, on: !likedA.has(k) }) },
        { label: "Ajouter à une playlist…", icon: "list-music", run: () => setTimeout(() => playlistPicker(x, y, a.tracks.map((t) => t.path))) }
      );
    }
  } else if (artistRow) {
    const ar = artistByName.get(artistRow.dataset.artist);
    if (ar) {
      const all = ar.albums.flatMap((a) => a.tracks);
      items = [
        { label: "Tout lire", icon: "play", run: () => playList(all) },
        { label: "Ajouter à une playlist…", icon: "list-music", run: () => setTimeout(() => playlistPicker(e.clientX, e.clientY, all.map((t) => t.path))) },
        "-",
        { label: "Changer la photo…", icon: "image", run: () => openEditor(all, { tab: "photo", title: ar.name, artist: ar.name }) },
        { label: `Modifier l'artiste (${plural(all.length, "titre", "titres")})…`, icon: "pencil", run: () => openEditor(all, { tab: "infos", title: ar.name }) },
        { label: "Renommer les fichiers…", icon: "folder", run: () => openEditor(all, { tab: "files", title: ar.name }) },
      ];
    }
  }
  if (!items) return;
  e.preventDefault();
  showMenu(e.clientX, e.clientY, items);
});

// F2 : modifier la sélection.
document.addEventListener("keydown", (e) => {
  if (e.key === "F2" && selected.size && !$("editor")) {
    e.preventDefault();
    openEditor(selectedTracks(), { tab: "infos" });
  }
});

// --- mise à jour de la bibliothèque après une modification ---------------------------------

function mergeChanged(ch, focusArtist) {
  if (ch.removed?.length) {
    const gone = new Set(ch.removed);
    tracks = tracks.filter((t) => !gone.has(t.path));
    for (const p of ch.removed) selected.delete(p);
  }
  if (ch.tracks?.length) {
    const byPath = new Map(ch.tracks.map((t) => [t.path, t]));
    tracks = tracks.filter((t) => !byPath.has(t.path)).concat(ch.tracks);
    for (const t of ch.tracks) coverVer.set(t.path, (coverVer.get(t.path) || 0) + 1);
  }
  index();
  if (route.view === "music" && !artistByName.has(route.artist)) {
    route.artist = focusArtist && artistByName.has(focusArtist) ? focusArtist : ch.tracks?.[0] ? artistOf(ch.tracks[0]) : undefined;
    route.album = undefined;
  }
  render();
  if (ch.errors?.length) toast(`${plural(ch.errors.length, "erreur", "erreurs")} : ${ch.errors.slice(0, 3).join(" · ")}`, "error");
}

// --- éditeur ----------------------------------------------------------------------------------

const FIELD_LABELS = {
  albumArtist: "Artiste de l'album",
  album: "Album",
  year: "Année",
  genre: "Genre",
  artist: "Artiste (pistes)",
  title: "Titre",
  track: "N°",
  disc: "Disque",
};
const fieldVal = (t, f) => {
  const v = t[f];
  if (f === "albumArtist") return t.albumArtist || "";
  return v === null || v === undefined ? "" : String(v);
};

let ed = null;

function openEditor(list, opts = {}) {
  if (!list.length) return;
  const sorted = sortAlbumTracks(list);
  const first = sorted[0];
  ed = {
    tracks: sorted,
    tab: opts.tab || "infos",
    title: opts.title || (sorted.length === 1 ? first.title : plural(sorted.length, "titre", "titres")),
    query: { artist: artistOf(first), album: first.album || "" },
    covers: null,
    coverPick: null,
    fixSource: "musicbrainz",
    releases: null,
    release: null,
    busy: "",
    artist: opts.artist || null,
    photos: null,
    photoPick: null,
  };
  if (ed.artist) ed.query.artist = ed.artist;
  closeEditor();
  const el = document.createElement("div");
  el.id = "editor";
  el.className = "editor-backdrop";
  el.innerHTML = `<div class="editor" role="dialog" aria-modal="true"></div>`;
  document.body.append(el);
  el.addEventListener("mousedown", (e) => e.target === el && closeEditor());
  renderEditor();
}

function closeEditor() {
  $("editor")?.remove();
}
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && $("editor")) {
    e.stopPropagation();
    closeEditor();
  }
}, true);

const TABS = [
  ["infos", "Infos", "pencil"],
  ["cover", "Pochette", "image"],
  ["fix", "Correction", "wand"],
  ["files", "Fichiers", "folder"],
];

function renderEditor() {
  const box = document.querySelector("#editor .editor");
  if (!box) return;
  const body = { infos: edInfos, cover: edCover, fix: edFix, files: edFiles, photo: edPhoto }[ed.tab]();
  const tabs = ed.artist ? [["photo", "Photo", "image"], ...TABS] : TABS;
  box.innerHTML = `
    <header class="ed-head">
      ${ed.artist && artistByName.has(ed.artist) ? artistImg(artistByName.get(ed.artist), "cover ed-thumb round") : img(ed.tracks[0].path, "cover ed-thumb")}
      <div style="min-width:0">
        <div class="ed-title">${esc(ed.title)}</div>
        <div class="hint">${plural(ed.tracks.length, "fichier", "fichiers")}</div>
      </div>
      <button class="icon-btn ed-close" data-ed="close" aria-label="Fermer">${icon("x")}</button>
    </header>
    <nav class="ed-tabs">${tabs.map(([id, label, ic]) => `<button data-ed-tab="${id}" class="${ed.tab === id ? "active" : ""}">${icon(ic)}${label}</button>`).join("")}</nav>
    <div class="ed-body">${body}</div>`;
  hydrateArtistPhotos();
}

document.addEventListener("click", (e) => {
  const ctl = e.target.closest("#editor [data-ed], #editor [data-ed-tab]");
  if (!ctl) return;
  if (ctl.dataset.edTab) {
    ed.tab = ctl.dataset.edTab;
    return renderEditor();
  }
  const fn = {
    close: closeEditor,
    saveInfos,
    searchCovers,
    pickLocalCover,
    applyCover,
    searchReleases,
    applyFix,
    applyRename,
    searchPhotos,
    applyPhoto,
    pickLocalPhoto,
    fixBack: () => {
      ed.release = null;
      renderEditor();
    },
  }[ctl.dataset.ed];
  if (fn) fn(ctl);
});

function busy(msg) {
  ed.busy = msg;
  renderEditor();
}

// ---- onglet Infos

function edInfos() {
  const multi = ed.tracks.length > 1;
  const common = multi ? ["albumArtist", "album", "year", "genre", "artist"] : ["title", "artist", "albumArtist", "album", "year", "genre", "track", "disc"];
  const input = (f) => {
    const vals = new Set(ed.tracks.map((t) => fieldVal(t, f)));
    const mixed = vals.size > 1;
    const v = mixed ? "" : [...vals][0];
    return `<label class="fld"><span>${FIELD_LABELS[f]}</span>
      <input data-common="${f}" value="${esc(v)}" placeholder="${mixed ? "Valeurs multiples" : ""}" ${mixed ? "data-mixed" : ""}></label>`;
  };
  const table = multi && ed.tracks.length <= 600
    ? `<div class="ed-table"><table>
        <thead><tr><th class="c-n">Disque</th><th class="c-n">N°</th><th>Titre</th><th>Artiste</th></tr></thead>
        <tbody>${ed.tracks
          .map(
            (t, i) => `<tr>
            <td><input data-row="${i}" data-f="disc" value="${esc(fieldVal(t, "disc"))}"></td>
            <td><input data-row="${i}" data-f="track" value="${esc(fieldVal(t, "track"))}"></td>
            <td><input data-row="${i}" data-f="title" value="${esc(t.title)}"></td>
            <td><input data-row="${i}" data-f="artist" value="${esc(t.artist)}"></td>
          </tr>`
          )
          .join("")}</tbody></table></div>`
    : "";
  return `<div class="ed-grid">${common.map(input).join("")}</div>
    ${multi ? `<p class="hint">Les champs « Valeurs multiples » ne sont modifiés que si tu les remplis.</p>` : ""}
    ${table}
    <footer class="ed-foot"><span class="hint">${esc(ed.busy)}</span><button class="btn" data-ed="close">Annuler</button><button class="btn primary" data-ed="saveInfos">Enregistrer</button></footer>`;
}

async function saveInfos() {
  const box = document.querySelector("#editor .editor");
  const common = {};
  box.querySelectorAll("[data-common]").forEach((inp) => {
    const f = inp.dataset.common;
    const orig = inp.hasAttribute("data-mixed") ? null : inp.defaultValue;
    if (inp.hasAttribute("data-mixed") ? inp.value.trim() !== "" : inp.value !== orig) common[f] = inp.value;
  });
  const per = ed.tracks.map(() => ({}));
  box.querySelectorAll("[data-row]").forEach((inp) => {
    if (inp.value !== inp.defaultValue) per[Number(inp.dataset.row)][inp.dataset.f] = inp.value;
  });
  const edits = ed.tracks
    .map((t, i) => ({ path: t.path, fields: { ...common, ...per[i] } }))
    .filter((x) => Object.keys(x.fields).length);
  if (!edits.length) return closeEditor();
  busy("Enregistrement…");
  try {
    const ch = await invoke("edit_tags", { label: `Infos · ${ed.title}`, edits });
    const focus = common.albumArtist || undefined;
    closeEditor();
    mergeChanged(ch, focus);
    if (!ch.errors.length) toast(`${plural(ch.tracks.length, "fichier modifié", "fichiers modifiés")}`);
  } catch (err) {
    busy("");
    toast(String(err), "error");
  }
}

// ---- onglet Pochette

function queryFields() {
  return `<div class="ed-query">
    <label class="fld"><span>Artiste</span><input id="q-artist" value="${esc(ed.query.artist)}"></label>
    <label class="fld"><span>Album</span><input id="q-album" value="${esc(ed.query.album)}"></label>`;
}
function readQuery() {
  ed.query = { artist: $("q-artist").value.trim(), album: $("q-album").value.trim() };
}

function edCover() {
  const hits = ed.covers?.hits || [];
  const grid = ed.covers
    ? hits.length
      ? `<div class="cv-grid">${hits
          .map(
            (h, k) => `<button class="cv ${ed.coverPick === k ? "on" : ""}" data-cv="${k}">
              <img src="${esc(h.thumb)}" alt="" loading="lazy" onload="this.nextElementSibling.querySelector('.dim').textContent=''">
              <span class="cv-meta"><b>${esc(h.source)}</b> ${esc(h.title)}<br><span class="hint">${esc(h.artist)}${h.year ? " · " + esc(h.year) : ""}</span><span class="dim"></span></span>
            </button>`
          )
          .join("")}</div>`
      : `<div class="empty" style="padding:30px">Aucune pochette trouvée.</div>`
    : "";
  const errs = ed.covers?.errors?.length ? `<p class="hint">Sources indisponibles : ${esc(ed.covers.errors.join(" · "))}</p>` : "";
  return `<div class="cv-top">
      ${img(ed.tracks[0].path, "cover cv-current")}
      <div style="flex:1;min-width:0">
        ${queryFields()}<button class="btn primary" data-ed="searchCovers">${icon("search")}Rechercher</button></div>
        <p class="hint">Sources : ${settings.discogsToken ? "Discogs, " : ""}MusicBrainz, iTunes, Deezer.${settings.discogsToken ? "" : " Ajoute ton jeton Discogs dans les paramètres pour l'inclure."}</p>
      </div>
    </div>
    ${errs}${grid}
    <footer class="ed-foot"><span class="hint">${esc(ed.busy)}</span>
      <button class="btn" data-ed="pickLocalCover">${icon("folder")}Fichier sur le PC…</button>
      <button class="btn primary" data-ed="applyCover" ${ed.coverPick === null ? "disabled" : ""}>Appliquer à ${plural(ed.tracks.length, "fichier", "fichiers")}</button>
    </footer>`;
}

document.addEventListener("click", (e) => {
  const c = e.target.closest("#editor [data-cv]");
  if (!c) return;
  ed.coverPick = Number(c.dataset.cv);
  document.querySelectorAll("#editor .cv").forEach((x) => x.classList.toggle("on", x === c));
  document.querySelector('#editor [data-ed="applyCover"]').disabled = false;
});

async function searchCovers() {
  readQuery();
  busy("Recherche…");
  try {
    ed.covers = await invoke("search_covers", ed.query);
    ed.coverPick = null;
  } catch (err) {
    toast(String(err), "error");
  }
  busy("");
}

async function setCover(source) {
  busy("Téléchargement et intégration de la pochette…");
  try {
    const ch = await invoke("set_cover", { label: `Pochette · ${ed.title}`, paths: ed.tracks.map((t) => t.path), source });
    closeEditor();
    mergeChanged(ch);
    if (!ch.errors.length) toast("Pochette mise à jour");
  } catch (err) {
    busy("");
    toast(String(err), "error");
  }
}
function applyCover() {
  const h = ed.covers?.hits?.[ed.coverPick];
  if (h) setCover(h.url);
}
async function pickLocalCover() {
  const f = await invoke("pick_image");
  if (f) setCover(f);
}

// ---- onglet Photo (artiste)

function edPhoto() {
  const ar = artistByName.get(ed.artist);
  const hits = ed.photos?.hits || [];
  const grid = ed.photos
    ? hits.length
      ? `<div class="cv-grid">${hits
          .map(
            (h, k) => `<button class="cv ${ed.photoPick === k ? "on" : ""}" data-ph="${k}">
              <img src="${esc(h.thumb)}" alt="" loading="lazy">
              <span class="cv-meta"><b>${esc(h.source)}</b> ${esc(h.artist)}</span>
            </button>`
          )
          .join("")}</div>`
      : `<div class="empty" style="padding:30px">Aucune photo trouvée.</div>`
    : "";
  return `<div class="cv-top">
      ${ar ? artistImg(ar, "cover cv-current round") : ""}
      <div style="flex:1;min-width:0">
        <div class="ed-query" style="grid-template-columns:1fr auto">
          <label class="fld"><span>Artiste</span><input id="q-artist" value="${esc(ed.query.artist)}"></label>
          <button class="btn primary" data-ed="searchPhotos">${icon("search")}Rechercher</button>
        </div>
        <p class="hint">La photo est enregistrée en « artist.jpg » dans le dossier de l'artiste quand il en a un. Sans photo, l'app affiche la pochette d'un album.</p>
      </div>
    </div>
    ${ed.photos?.errors?.length ? `<p class="hint">Sources indisponibles : ${esc(ed.photos.errors.join(" · "))}</p>` : ""}
    ${grid}
    <footer class="ed-foot"><span class="hint">${esc(ed.busy)}</span>
      <button class="btn" data-ed="pickLocalPhoto">${icon("folder")}Fichier sur le PC…</button>
      <button class="btn primary" data-ed="applyPhoto" ${ed.photoPick === null ? "disabled" : ""}>Utiliser cette photo</button>
    </footer>`;
}

document.addEventListener("click", (e) => {
  const c = e.target.closest("#editor [data-ph]");
  if (!c) return;
  ed.photoPick = Number(c.dataset.ph);
  document.querySelectorAll("#editor .cv").forEach((x) => x.classList.toggle("on", x === c));
  document.querySelector('#editor [data-ed="applyPhoto"]').disabled = false;
});

async function searchPhotos() {
  ed.query.artist = $("q-artist").value.trim();
  busy("Recherche…");
  try {
    ed.photos = await invoke("search_artist_images", { name: ed.query.artist });
    ed.photoPick = null;
  } catch (err) {
    toast(String(err), "error");
  }
  busy("");
}

async function setArtistPhoto(source) {
  busy("Enregistrement de la photo…");
  try {
    await invoke("set_artist_image", { name: ed.artist, track: ed.tracks[0].path, source });
    artistVer.set(ed.artist, (artistVer.get(ed.artist) || 0) + 1);
    closeEditor();
    render();
    toast("Photo mise à jour");
  } catch (err) {
    busy("");
    toast(String(err), "error");
  }
}
function applyPhoto() {
  const h = ed.photos?.hits?.[ed.photoPick];
  if (h) setArtistPhoto(h.url);
}
async function pickLocalPhoto() {
  const f = await invoke("pick_image");
  if (f) setArtistPhoto(f);
}

// ---- onglet Correction

function edFix() {
  if (ed.release) return edFixDiff();
  const list = ed.releases
    ? ed.releases.length
      ? `<div class="rel-list">${ed.releases
          .map(
            (r, k) => `<button class="rel" data-rel="${k}">
              <b>${esc(r.title)}</b> <span class="hint">· ${esc(r.artist)}</span><br>
              <span class="hint">${[r.year, r.country, r.label, r.format, r.trackCount ? plural(r.trackCount, "piste", "pistes") : ""].filter(Boolean).map(esc).join(" · ")}</span>
            </button>`
          )
          .join("")}</div>`
      : `<div class="empty" style="padding:30px">Aucun résultat.</div>`
    : "";
  return `<div class="seg">
      <button data-src="musicbrainz" class="${ed.fixSource === "musicbrainz" ? "on" : ""}">MusicBrainz</button>
      <button data-src="discogs" class="${ed.fixSource === "discogs" ? "on" : ""}">Discogs</button>
    </div>
    <div class="cv-top" style="margin-top:12px"><div style="flex:1">${queryFields()}<button class="btn primary" data-ed="searchReleases">${icon("search")}Rechercher</button></div></div></div>
    ${list}
    <footer class="ed-foot"><span class="hint">${esc(ed.busy)}</span></footer>`;
}

document.addEventListener("click", async (e) => {
  const src = e.target.closest("#editor [data-src]");
  if (src) {
    ed.fixSource = src.dataset.src;
    ed.releases = null;
    return renderEditor();
  }
  const rel = e.target.closest("#editor [data-rel]");
  if (!rel) return;
  const r = ed.releases[Number(rel.dataset.rel)];
  busy("Chargement de la fiche…");
  try {
    ed.release = await invoke("release_details", { source: r.source, id: r.id });
  } catch (err) {
    toast(String(err), "error");
  }
  busy("");
});

async function searchReleases() {
  readQuery();
  busy("Recherche…");
  try {
    ed.releases = await invoke("search_releases", { source: ed.fixSource, ...ed.query });
  } catch (err) {
    ed.releases = null;
    toast(String(err), "error");
  }
  busy("");
}

const FIX_OPTS = [
  ["album", "Album", true],
  ["albumArtist", "Artiste de l'album", true],
  ["year", "Année", true],
  ["genre", "Genre", true],
  ["title", "Titres", true],
  ["numbers", "Numéros de piste et de disque", true],
  ["artist", "Artistes des pistes", false],
  ["cover", "Pochette", false],
];

function edFixDiff() {
  const R = ed.release;
  const n = Math.min(ed.tracks.length, R.tracks.length);
  const warn = ed.tracks.length !== R.tracks.length
    ? `<p class="warn-box">L'album contient ${plural(ed.tracks.length, "piste", "pistes")}, la fiche en a ${R.tracks.length} : les pistes sont associées dans l'ordre, vérifie la correspondance.</p>`
    : "";
  const cmp = (a, b) => (String(a ?? "") === String(b ?? "") ? "" : "diff");
  const t0 = ed.tracks[0];
  const head = [
    ["Album", t0.album, R.title],
    ["Artiste de l'album", t0.albumArtist, R.artist],
    ["Année", t0.year, R.year],
    ["Genre", t0.genre, R.genre],
  ];
  return `<button class="btn small" data-ed="fixBack">${icon("chevron-left")}Résultats</button>
    <h3 class="ed-h3">${esc(R.title)} <span class="hint">· ${esc(R.artist)} · ${esc(R.source)}</span></h3>
    ${warn}
    <div class="fix-opts">${FIX_OPTS.map(([id, label, on]) => (id === "cover" && !R.cover ? "" : `<label class="chk"><input type="checkbox" data-fix="${id}" ${on ? "checked" : ""}> ${label}</label>`)).join("")}</div>
    <div class="ed-table"><table class="diff">
      <thead><tr><th></th><th>Actuel</th><th>Proposé</th></tr></thead>
      <tbody>
        ${head.map(([l, a, b]) => `<tr class="${cmp(a, b)}"><td class="hint">${l}</td><td>${esc(a)}</td><td>${esc(b)}</td></tr>`).join("")}
        ${ed.tracks
          .slice(0, Math.max(ed.tracks.length, 0))
          .map((t, i) => {
            const p = R.tracks[i];
            if (!p) return `<tr><td class="hint">${t.track ?? ""}</td><td>${esc(t.title)}</td><td class="hint">—</td></tr>`;
            const d = cmp(t.title, p.title) || cmp(t.track, p.track) || cmp(t.disc || 1, p.disc) ? "diff" : "";
            return `<tr class="${d}"><td class="hint">${p.disc > 1 || R.tracks.some((x) => x.disc > 1) ? p.disc + "-" : ""}${p.track}</td>
              <td>${esc(t.title)}<br><span class="hint">${esc(t.artist)}</span></td>
              <td>${esc(p.title)}<br><span class="hint">${esc(p.artist)}</span></td></tr>`;
          })
          .join("")}
      </tbody></table></div>
    <footer class="ed-foot"><span class="hint">${esc(ed.busy)}</span><button class="btn primary" data-ed="applyFix">Appliquer à ${plural(n, "fichier", "fichiers")}</button></footer>`;
}

async function applyFix() {
  const R = ed.release;
  const on = {};
  document.querySelectorAll("#editor [data-fix]").forEach((c) => (on[c.dataset.fix] = c.checked));
  const edits = ed.tracks.slice(0, R.tracks.length).map((t, i) => {
    const p = R.tracks[i];
    const f = {};
    if (on.album && R.title) f.album = R.title;
    if (on.albumArtist && R.artist) f.albumArtist = R.artist;
    if (on.year && R.year) f.year = R.year;
    if (on.genre && R.genre) f.genre = R.genre;
    if (on.title && p.title) f.title = p.title;
    if (on.artist && p.artist) f.artist = p.artist;
    if (on.numbers) {
      f.track = String(p.track);
      f.disc = String(p.disc);
    }
    return { path: t.path, fields: f };
  });
  busy("Enregistrement…");
  try {
    const label = `Correction ${R.source} · ${R.title}`;
    let ch = await invoke("edit_tags", { label, edits });
    if (on.cover && R.cover) {
      busy("Téléchargement de la pochette…");
      const c2 = await invoke("set_cover", { label: `Pochette ${R.source} · ${R.title}`, paths: ed.tracks.map((t) => t.path), source: R.cover });
      ch = { removed: [], tracks: c2.tracks.length ? c2.tracks : ch.tracks, errors: ch.errors.concat(c2.errors) };
    }
    closeEditor();
    mergeChanged(ch, on.albumArtist ? R.artist : undefined);
    if (!ch.errors.length) toast("Infos corrigées");
  } catch (err) {
    busy("");
    toast(String(err), "error");
  }
}

// ---- onglet Fichiers

const TOKENS = ["albumartist", "artist", "album", "year", "genre", "disc", "track", "title"];
const cleanName = (s) =>
  String(s ?? "")
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[. ]+$/, "") || "Inconnu";

function plannedPath(t, pattern) {
  const multiDisc = (albumByKey.get(albumKeyOf(t))?.tracks || [t]).some((x) => (x.disc || 1) > 1);
  const val = {
    albumartist: artistOf(t),
    artist: t.artist || "Artiste inconnu",
    album: t.album || "Album inconnu",
    year: t.year || "",
    genre: t.genre || "",
    disc: multiDisc ? `${t.disc || 1}-` : "",
    track: t.track ? String(t.track).padStart(2, "0") : "",
    title: t.title,
  };
  const segs = pattern
    .split(/[\\/]/)
    .map((seg) =>
      cleanName(
        seg
          .replace(/\{(\w+)\}/g, (m, k) => (k in val ? String(val[k]).replace(/[\\/]/g, "_") : m))
          // Nettoie les séparateurs orphelins quand un champ est vide (« - Album », « 01 - »).
          .replace(/^\s*[-–·]\s*/, "")
          .replace(/\s*[-–·]\s*$/, "")
      )
    );
  const ext = t.path.slice(t.path.lastIndexOf("."));
  const r = rootOf(t.path);
  const s = sep(r);
  return r.replace(/[\\/]+$/, "") + s + segs.join(s) + ext.toLowerCase();
}

function edFiles() {
  if (!root) return `<div class="empty">Choisis d'abord le dossier de ta bibliothèque dans les paramètres.</div>`;
  const pattern = ed.pattern ?? settings.renamePattern;
  ed.pattern = pattern;
  const rel = (p) => p.slice(rootOf(p).length).replace(/^[\\/]/, "");
  const plan = ed.tracks.map((t) => [t.path, plannedPath(t, pattern)]);
  const changed = plan.filter(([a, b]) => a !== b);
  const dup = new Set();
  const seen = new Set();
  for (const [, b] of plan) (seen.has(b.toLowerCase()) ? dup : seen).add(b.toLowerCase());
  ed.plan = changed;
  return `<label class="fld"><span>Modèle</span><input id="pattern" value="${esc(pattern)}" spellcheck="false"></label>
    <p class="hint">Champs : ${TOKENS.map((k) => `<code>{${k}}</code>`).join(" ")} — « / » crée un dossier. {disc} n'apparaît que pour les albums à plusieurs disques.</p>
    ${dup.size ? `<p class="warn-box">Plusieurs fichiers obtiendraient le même nom : ajoute {track} ou {disc} au modèle.</p>` : ""}
    <div class="ed-table"><table class="ren">
      <thead><tr><th>Actuel</th><th>Nouveau</th></tr></thead>
      <tbody>${plan
        .map(([a, b]) => `<tr class="${a === b ? "same" : "diff"}"><td>${esc(rel(a))}</td><td>${esc(rel(b))}</td></tr>`)
        .join("")}</tbody></table></div>
    <footer class="ed-foot"><span class="hint">${esc(ed.busy) || (changed.length ? plural(changed.length, "fichier à renommer", "fichiers à renommer") : "Tout est déjà rangé ainsi")}</span>
      <button class="btn primary" data-ed="applyRename" ${!changed.length || dup.size ? "disabled" : ""}>Renommer</button></footer>`;
}

let patternTimer;
document.addEventListener("input", (e) => {
  if (e.target.id !== "pattern") return;
  clearTimeout(patternTimer);
  patternTimer = setTimeout(() => {
    const pos = e.target.selectionStart;
    ed.pattern = e.target.value;
    renderEditor();
    const inp = $("pattern");
    inp.focus();
    inp.setSelectionRange(pos, pos);
  }, 250);
});

async function applyRename() {
  if (!ed.plan?.length) return;
  busy("Renommage…");
  settings.renamePattern = ed.pattern;
  saveTools();
  try {
    const ch = await invoke("move_files", { label: `Renommage · ${ed.title}`, moves: ed.plan });
    closeEditor();
    mergeChanged(ch);
    if (!ch.errors.length) toast(`${plural(ch.tracks.length, "fichier renommé", "fichiers renommés")}`);
  } catch (err) {
    busy("");
    toast(String(err), "error");
  }
}

// --- paramètres : services en ligne, fichiers, historique -------------------------------------

let settings = { discogsToken: "", renamePattern: "{albumartist}/{year} - {album}/{disc}{track} - {title}", coverFile: true };
let historyCache = null;

function saveTools() {
  invoke("set_tools", { discogsToken: settings.discogsToken, renamePattern: settings.renamePattern, coverFile: settings.coverFile });
}



document.addEventListener("change", (e) => {
  const id = e.target.id;
  if (id === "discogs-token") settings.discogsToken = e.target.value.trim();
  else if (id === "cover-file") settings.coverFile = e.target.checked;
  else if (id === "default-pattern") settings.renamePattern = e.target.value;
  else return;
  saveTools();
  toast("Réglage enregistré");
});

// --- réglages Lecture et Sortie ------------------------------------------------------------

const opt = (v, cur, label) => `<option value="${v}" ${String(v) === String(cur) ? "selected" : ""}>${esc(label)}</option>`;
const fmtDbSigned = (v) => (v > 0 ? "+" : "") + String(v).replace(".", DEC) + " dB";
const fmtSec = (v) => (v ? String(v).replace(".", DEC) + " s" : tr("désactivé"));
const fmtBal = (v) => (Math.abs(v) < 0.005 ? tr("centre") : tr(v < 0 ? "gauche" : "droite") + " " + Math.round(Math.abs(v) * 100) + " %");



function readAudioInput(el) {
  const k = el.dataset.audio;
  if (el.type === "checkbox") return el.checked;
  if (["replaygain", "endMode"].includes(k)) return el.value;
  const v = Number(el.value);
  return k === "maxVolume" ? v / 100 : v;
}
function paintAudioVal(el) {
  const out = el.parentElement.querySelector(".set-val");
  if (!out) return;
  const v = Number(el.value);
  out.textContent = { db: fmtDbSigned, sec: fmtSec, pct: (x) => x + " %", bal: fmtBal }[el.dataset.fmt]?.(v) ?? v;
}
document.addEventListener("input", (e) => {
  if (e.target.dataset?.audio && e.target.type === "range") paintAudioVal(e.target);
});
document.addEventListener("change", (e) => {
  const el = e.target;
  if (el.id === "resume-on" || el.id === "shuffle-on") {
    resumeOn = $("resume-on").checked;
    shuffleOn = $("shuffle-on").checked;
    return savePlayPrefs();
  }
  if (!el.dataset?.audio) return;
  audio[el.dataset.audio] = readAudioInput(el);
  saveAudio();
});
document.addEventListener("click", (e) => {
  if (!e.target.closest("[data-balance-reset]")) return;
  audio.balance = 0;
  const r = document.querySelector('[data-audio="balance"]');
  if (r) {
    r.value = 0;
    paintAudioVal(r);
  }
  saveAudio();
});

// Last.fm
document.addEventListener("click", async (e) => {
  if (e.target.closest("[data-lf-login]")) {
    const v = (id) => $(id).value.trim();
    if (!v("lf-key") || !v("lf-secret") || !v("lf-user") || !$("lf-pass").value) return toast("Remplis les quatre champs.", "error");
    try {
      await invoke("lastfm_login", { apiKey: v("lf-key"), apiSecret: v("lf-secret"), user: v("lf-user"), password: $("lf-pass").value });
      const st = await invoke("get_settings");
      settings.lastfm = st.lastfm;
      toast("Connecté à Last.fm : tes écoutes seront envoyées");
      render();
    } catch (err) {
      toast("Last.fm : " + err, "error");
    }
  } else if (e.target.closest("[data-lf-logout]")) {
    await invoke("lastfm_set", { scrobble: false, logout: true });
    settings.lastfm = { ...settings.lastfm, session: "", user: "", scrobble: false };
    render();
  }
});
document.addEventListener("change", (e) => {
  if (e.target.id !== "lf-scrobble") return;
  settings.lastfm.scrobble = e.target.checked;
  invoke("lastfm_set", { scrobble: e.target.checked, logout: false });
});

// --- Système : démarrage de Windows, zone de notification, sauvegarde ------------------------

let sysPrefs = { autostart: false, trayClose: false, smtc: false };
document.addEventListener("change", async (e) => {
  if (e.target.id !== "sys-autostart" && e.target.id !== "sys-tray") return;
  sysPrefs = { ...sysPrefs, autostart: $("sys-autostart").checked, trayClose: $("sys-tray").checked };
  try {
    await invoke("set_system", { autostart: sysPrefs.autostart, trayClose: sysPrefs.trayClose });
  } catch (err) {
    toast(String(err), "error");
  }
});
document.addEventListener("click", async (e) => {
  if (e.target.closest("[data-backup-export]")) {
    try {
      const p = await invoke("export_backup");
      if (p) toast("Sauvegarde enregistrée : " + p);
    } catch (err) {
      toast(String(err), "error");
    }
  } else if (e.target.closest("[data-backup-import]")) {
    try {
      if (await invoke("import_backup")) {
        toast("Sauvegarde restaurée : rechargement…");
        setTimeout(() => location.reload(), 800);
      }
    } catch (err) {
      toast(String(err), "error");
    }
  }
});

// --- app téléphone : serveur, appairage, appareils ----------------------------------------

let phoneCache = null;
function refreshPhone(p) {
  phoneCache = p;
  const el = $("phone");
  if (el) el.outerHTML = phoneSection();
}
function phoneSection() {
  if (!phoneCache) invoke("phone_status").then(refreshPhone);
  const p = phoneCache;
  if (!p) return `<section class="section" id="phone"><p class="hint" style="margin:0">Chargement…</p></section>`;
  const date = (s) => new Date(s * 1000).toLocaleDateString(LOCALE, { dateStyle: "medium" });
  const lan = p.addresses.filter((a) => !a.startsWith("100."));
  const ts = p.addresses.filter((a) => a.startsWith("100."));
  return `<div id="phone" class="settings">
    <section class="section">
      <div class="section-head">
        <h2>App Android</h2>
        <label class="switch" title="Partager la bibliothèque avec le téléphone"><input type="checkbox" id="phone-on" ${p.enabled ? "checked" : ""}><span></span></label>
      </div>
      <p class="hint" style="margin:0">Quand c'est activé, ton téléphone peut parcourir ta bibliothèque, écouter tes fichiers, en télécharger pour les écouter hors ligne, et piloter la lecture sur ce PC. Seuls les appareils appairés y ont accès.</p>
      ${p.error ? `<p class="phone-err">${esc(p.error)}</p>` : ""}
      ${
        p.enabled && p.running
          ? `<div class="phone-grid">
              <div class="field"><span class="label">Adresse à la maison</span>
                <div class="path">${lan.length ? lan.map((a) => esc(`${a}:${p.port}`)).join("<br>") : "Pas de réseau détecté"}</div></div>
              <div class="field"><span class="label">Adresse hors de chez toi (Tailscale)</span>
                <div class="path ${ts.length ? "" : "none"}">${ts.length ? ts.map((a) => esc(`${a}:${p.port}`)).join("<br>") : "Tailscale n'est pas installé"}</div></div>
            </div>
            <div class="field"><span class="label">Code d'appairage</span>
              <div class="pair-row"><span class="pair-code">${esc(p.code.slice(0, 3))} ${esc(p.code.slice(3))}</span>
              <button class="btn small" data-phone-code>Nouveau code</button></div>
              <span class="hint">Dans l'app, tape l'adresse « à la maison » puis ce code. Il ne sert qu'une fois ; l'adresse Tailscale est retenue toute seule.</span></div>`
          : ""
      }
      <label class="fld phone-port"><span>Port</span><input id="phone-port" type="number" min="1024" max="65535" value="${p.port}"></label>
    </section>
    <section class="section">
      <h2>Appareils appairés</h2>
      ${
        p.devices.length
          ? `<div class="hist">${p.devices
              .map(
                (d, i) => `<div class="hist-row"><span><b>${esc(d.name || tr("Téléphone"))}</b><br><span class="hint">Appairé le ${date(d.added)}</span></span>
                  <button class="btn small" data-phone-revoke="${i}">Retirer</button></div>`
              )
              .join("")}</div>`
          : `<p class="hint" style="margin:0">Aucun appareil pour l'instant.</p>`
      }
    </section>
  </div>`;
}
async function savePhone() {
  const on = $("phone-on")?.checked ?? false;
  const port = +($("phone-port")?.value || 47800);
  refreshPhone(await invoke("set_phone", { enabled: on, port }));
}
document.addEventListener("change", (e) => {
  if (e.target.id === "phone-on" || e.target.id === "phone-port") savePhone();
});
document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-phone-code],[data-phone-revoke]");
  if (!b) return;
  if (b.dataset.phoneRevoke != null) {
    await invoke("phone_revoke", { index: +b.dataset.phoneRevoke });
    toast("Appareil retiré : il n'a plus accès à ta bibliothèque.");
  } else await invoke("phone_new_code");
  refreshPhone(await invoke("phone_status"));
});
listen("phone-changed", async () => {
  const p = await invoke("phone_status");
  phoneCache = p;
  if ($("phone")) refreshPhone(p);
  toast("Nouveau téléphone appairé");
});

function historySection() {
  if (!historyCache) {
    invoke("history").then((h) => {
      historyCache = h || [];
      const el = $("history");
      if (el) el.outerHTML = historySection();
    });
  }
  const h = historyCache || [];
  const date = (s) => new Date(s * 1000).toLocaleString(LOCALE, { dateStyle: "short", timeStyle: "short" });
  const count = (e) =>
    [e.tags.length && plural(e.tags.length, "fichier modifié", "fichiers modifiés"), e.covers.length && plural(e.covers.length, "pochette", "pochettes"), e.moves.length && plural(e.moves.length, "déplacement", "déplacements")]
      .filter(Boolean)
      .join(", ");
  return `<section class="section" id="history">
    <h2>Historique des modifications</h2>
    ${
      h.length
        ? `<div class="hist">${h
            .slice(0, 60)
            .map(
              (e) => `<div class="hist-row ${e.undone ? "undone" : ""}">
                <span><b>${esc(e.label)}</b><br><span class="hint">${date(e.date)} · ${count(e)}${e.undone ? " · annulé" : ""}</span></span>
                ${e.undone ? "" : `<button class="btn small" data-undo="${e.id}">Annuler</button>`}
              </div>`
            )
            .join("")}</div>`
        : `<p class="hint" style="margin:0">${historyCache ? "Aucune modification pour l'instant." : "Chargement…"}</p>`
    }
  </section>`;
}

document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-undo]");
  if (!b) return;
  b.disabled = true;
  try {
    const ch = await invoke("undo", { id: Number(b.dataset.undo) });
    historyCache = null;
    mergeChanged(ch);
    if (!ch.errors.length) toast("Modification annulée");
  } catch (err) {
    toast(String(err), "error");
  }
});

// L'historique est relu à chaque ouverture des paramètres.
const _go = go;
go = function (r, push) {
  if (r.view === "settings") historyCache = null;
  return _go(r, push);
};

// --- écran d'accueil (logo animé) -------------------------------------------------------------

const splashStart = performance.now();
let splashGone = false;
function hideSplash() {
  if (splashGone) return;
  splashGone = true;
  const el = $("splash");
  if (!el) return;
  // On laisse l'animation du logo se jouer en entier.
  const wait = Math.max(0, 2100 - (performance.now() - splashStart));
  setTimeout(() => {
    el.classList.add("out");
    setTimeout(() => el.remove(), 700);
  }, wait);
}
setTimeout(hideSplash, 6000); // au cas où le démarrage traînerait

// --- démarrage ----------------------------------------------------------------------------

(async () => {
  const s = await invoke("get_settings");
  output = s.output;
  document.querySelector(`input[name="mode"][value="${output.exclusive ? "exclusive" : "shared"}"]`).checked = true;
  $("volume").value = Math.round(s.volume * 100);
  eq = { ...eq, ...(s.eq || {}) };
  shortcuts = { ...DEFAULT_SHORTCUTS, ...(s.shortcuts || {}) };
  accentPref = s.accent === "windows" ? "windows" : "coral";
  ambientSettings = { auto: !!s.ambientAuto, delay: s.ambientDelay || 300 };
  audio = { ...audio, ...(s.audio || {}) };
  shuffleOn = !!s.shuffle;
  resumeOn = s.resume ?? true;
  paintModes();
  applyMode(s.theme);
  refreshWindowsAccent();
  settings = { discogsToken: s.discogsToken || "", renamePattern: s.renamePattern || settings.renamePattern, coverFile: s.coverFile ?? true, lastfm: s.lastfm || {} };
  renderEq();
  root = s.libraryFolder;
  extraRoots = s.extraFolders || [];
  libPrefs = { watch: s.watch ?? true, ignoreExt: s.ignoreExt || [] };
  ui = { ...ui, ...(s.ui || {}) };
  // La langue est lue avant le chargement (localStorage) ; on la resynchronise si besoin (sauvegarde restaurée…).
  if (ui.lang && ui.lang !== LANG_PREF) {
    try {
      localStorage.setItem("lang", ui.lang);
      return location.reload();
    } catch {}
  }
  invoke("set_tray_lang", { en: LANG === "en" }).catch(() => {});
  applyUi();
  invoke("get_collections").then(setColl);
  invoke("get_stats").then((st) => (stats = { plays: {}, last: {}, history: [], ...st }));
  invoke("get_system").then((x) => (sysPrefs = x));
  go({ view: ["music", "albums", "folders", "playlists"].includes(ui.startView) ? ui.startView : "music" }, false);
  if (root) {
    const cached = await invoke("load_library");
    if (cached.length) {
      setLibrary(cached);
      if (resumeOn) {
        const ses = await invoke("restore_session").catch(() => null);
        if (ses) {
          queue = ses.paths.map((p) => trackByPath.get(p)).filter(Boolean);
          renderRight();
        }
      }
    } else scan(root);
  } else {
    setLibrary([]);
  }
  hideSplash();
})();
