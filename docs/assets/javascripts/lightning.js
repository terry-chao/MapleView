/* ==========================================================================
   MapleView 官网 —— 「闪电预览」演示台
   --------------------------------------------------------------------------
   这里刻意复刻桌面端的那套策略，而不是做一个假的进度条：

     · 解码放在 Web Worker 池里            （对应 app 的解码线程池）
     · 切图用单调递增的版本号做取消        （对应 loader.rs 的原子版本号）
     · 当前图的邻居提前预取进缓存
     · 缓存按「字节」而不是张数计费，超预算 LRU 淘汰（对应 cache.rs）
     · 先按视口分辨率解一张预览图，放大到 100% 以上再后台补全分辨率

   所有毫秒数都是这台机器上的真实测量值，没有任何写死的数字。
   ========================================================================== */

(function () {
  "use strict";

  var root = document.querySelector("[data-mv-demo]");
  if (!root) return;

  var $ = function (sel) { return root.querySelector(sel); };

  var viewport = $("[data-mv-viewport]");
  var canvas = $("[data-mv-canvas]");
  var ctx = canvas ? canvas.getContext("2d", { alpha: false }) : null;
  var strip = $("[data-mv-strip]");
  var spark = $("[data-mv-spark]");

  var ui = {
    empty: $("[data-mv-empty]"),
    spinner: $("[data-mv-spinner]"),
    path: $("[data-mv-path]"),
    count: $("[data-mv-count]"),
    format: $("[data-mv-format]"),
    dims: $("[data-mv-dims]"),
    title: $("[data-mv-title]"),
    latency: $("[data-mv-latency]"),
    latencyValue: $("[data-mv-latency-value]"),
    badge: $("[data-mv-badge]"),
    zoom: $("[data-mv-zoom]"),
    drop: $("[data-mv-drop]"),
    hits: $("[data-mv-hits]"),
    misses: $("[data-mv-misses]"),
    avg: $("[data-mv-avg]"),
    bytes: $("[data-mv-bytes]"),
    open: $("[data-mv-open]"),
    toggles: Array.prototype.slice.call(root.querySelectorAll("[data-mv-mode]")),
    stats: Array.prototype.slice.call(root.querySelectorAll("[data-count]")),
    steps: {}
  };

  Array.prototype.forEach.call(root.querySelectorAll("[data-mv-step]"), function (el) {
    ui.steps[el.getAttribute("data-mv-step")] = el;
  });

  if (!canvas || !ctx || !window.MV_CORPUS) {
    if (ui.empty) ui.empty.textContent = "演示无法启动：浏览器缺少 Canvas 支持。";
    return;
  }
  if (location.protocol === "file:") {
    // Worker / fetch 在 file:// 下会被浏览器拦掉，直接说明比留个空白框体面。
    if (ui.empty) ui.empty.innerHTML = "本地演示需要 http 环境，请先运行 <code>mkdocs serve</code>。";
    return;
  }

  var REDUCED = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  var BASE = new URL("assets/demo/", document.baseURI).href;
  var CACHE_BUDGET = 192 * 1024 * 1024;
  var HISTORY = 48;

  var state = {
    items: [],
    index: -1,
    mode: "fast",
    seq: 0,
    cache: new Map(),
    cacheBytes: 0,
    hits: 0,
    misses: 0,
    cancels: 0,
    latencies: [],
    run: { n: 0, sum: 0 },
    frame: null,          // { entry, kind, owned } —— 当前真正画在屏幕上的那一张
    view: { scale: 1, tx: 0, ty: 0, fit: 1, mode: "fit" },
    drag: null,
    ready: false,
    nextId: 1
  };

  /* -------------------------------------------------------------- 小工具 -- */

  function deviceScale() {
    return Math.min(window.devicePixelRatio || 1, 2);
  }

  function clamp(v, lo, hi) {
    return v < lo ? lo : v > hi ? hi : v;
  }

  function formatBytes(n) {
    if (!n) return "0 B";
    if (n >= 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(1) + " GiB";
    if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(0) + " MiB";
    if (n >= 1024) return (n / 1024).toFixed(0) + " KiB";
    return n + " B";
  }

  function extension(name) {
    var m = /\.([a-z0-9]+)$/i.exec(name || "");
    return m ? m[1].toUpperCase() : "IMG";
  }

  /* ---------------------------------------------------------- Worker 池 -- */

  var WORKER_SRC = [
    "'use strict';",
    "self.onmessage = async function (ev) {",
    "  var msg = ev.data, id = msg.id, url = msg.url, resize = msg.resize;",
    "  var t0 = performance.now();",
    "  var bytes = 0, fetchMs = 0, decodeMs = 0, probeMs = 0;",
    "  var bitmap = null, width = 0, height = 0, error = null;",
    "  try {",
    // 用默认缓存策略而不是 force-cache：图片是同名覆盖发布的，
    // 强制读缓存会在重新部署后拿到旧图，和 manifest 里的尺寸对不上。
    "    var res = await fetch(url, { cache: 'default' });",
    "    if (!res.ok) throw new Error('HTTP ' + res.status);",
    "    var buf = await res.arrayBuffer();",
    "    bytes = buf.byteLength;",
    "    fetchMs = performance.now() - t0;",
    // 文件头探测：WebCodecs 的 ImageDecoder 能不解码就读出真实像素尺寸，
    // 对应 core 里的 probe_size（只读文件头，不展开像素）。
    "    if (msg.probe && self.ImageDecoder) {",
    "      try {",
    "        var tp = performance.now();",
    "        var type = res.headers.get('content-type') || 'application/octet-stream';",
    "        var dec = new ImageDecoder({ data: buf, type: type });",
    "        await dec.tracks.ready;",
    "        var track = dec.tracks.selectedTrack;",
    "        width = track.codedWidth; height = track.codedHeight;",
    "        dec.close();",
    "        probeMs = performance.now() - tp;",
    "      } catch (e) { width = 0; height = 0; }",
    "    }",
    "    var opt = {};",
    "    if (resize && resize.w > 0 && resize.h > 0) {",
    "      opt.resizeWidth = resize.w;",
    "      opt.resizeHeight = resize.h;",
    "      opt.resizeQuality = resize.quality || 'high';",
    "    }",
    "    var t1 = performance.now();",
    "    bitmap = await createImageBitmap(new Blob([buf]), opt);",
    "    decodeMs = performance.now() - t1;",
    "    if (!width) { width = bitmap.width; height = bitmap.height; }",
    "  } catch (err) {",
    "    error = String((err && err.message) || err);",
    "  }",
    "  self.postMessage({",
    "    id: id, ok: !error, error: error, bitmap: bitmap,",
    "    width: width, height: height, bytes: bytes,",
    "    fetchMs: fetchMs, decodeMs: decodeMs, probeMs: probeMs",
    "  }, bitmap ? [bitmap] : []);",
    "};"
  ].join("\n");

  var workerUrl = URL.createObjectURL(new Blob([WORKER_SRC], { type: "text/javascript" }));
  var POOL = Math.max(2, Math.min(4, navigator.hardwareConcurrency || 4));
  var slots = [];
  var queue = [];
  var pending = new Map();   // key -> job，排队中或正在解的都算
  var jobId = 0;

  for (var s = 0; s < POOL; s++) {
    (function () {
      var slot = { busy: false, job: null, worker: new Worker(workerUrl) };
      slot.worker.onmessage = function (ev) { onWorkerMessage(slot, ev.data); };
      slot.worker.onerror = function () {
        var job = slot.job;
        slot.busy = false;
        slot.job = null;
        if (job) { pending.delete(job.key); job.settle({ ok: false, error: "worker 启动失败" }); }
        pump();
      };
      slots.push(slot);
    })();
  }

  function onWorkerMessage(slot, data) {
    var job = slot.job;
    slot.busy = false;
    slot.job = null;
    if (job) {
      pending.delete(job.key);
      if (job.cancelled) {
        if (data.bitmap) { data.bitmap.close(); data.bitmap = null; }
        data.ok = false;
      }
      data.cancelled = job.cancelled;
      data.resize = job.resize;
      job.settle(data);
    }
    pump();
  }

  function pump() {
    queue.sort(function (a, b) { return a.priority - b.priority || a.id - b.id; });
    for (var i = 0; i < slots.length; i++) {
      var slot = slots[i];
      if (slot.busy) continue;
      var job = null;
      while (queue.length) {
        var cand = queue.shift();
        if (cand.cancelled) { pending.delete(cand.key); continue; }
        job = cand;
        break;
      }
      if (!job) return;
      slot.busy = true;
      slot.job = job;
      slot.worker.postMessage({ id: job.id, url: job.url, resize: job.resize, probe: job.probe });
    }
  }

  function request(item, kind, opts) {
    opts = opts || {};
    var key = item.id + "|" + kind;
    if (pending.has(key)) return pending.get(key);
    var job = {
      id: ++jobId,
      key: key,
      url: item.url,
      resize: kind === "full" ? null : targetFor(item, kind),
      probe: kind === "probe",
      priority: typeof opts.priority === "number" ? opts.priority : 1,
      seq: typeof opts.seq === "number" ? opts.seq : -1,
      cancelled: false,
      settle: null,
      promise: null
    };
    job.promise = new Promise(function (resolve) { job.settle = resolve; });
    pending.set(key, job);
    queue.push(job);
    pump();
    return job;
  }

  /* ------------------------------------------------------------ 目标尺寸 -- */

  function targetFor(item, kind) {
    if (kind === "full" || kind === "probe") return null;
    var natW = item.w || 0;
    var natH = item.h || 0;
    if (!natW || !natH) return null;
    if (kind === "thumb") {
      var tw = Math.min(320, natW);
      return { w: tw, h: Math.max(1, Math.round((tw / natW) * natH)), quality: "low" };
    }
    // 预览图：按视口分辨率解码，再留一点余量给轻微放大就够。
    var dpr = deviceScale();
    var boxW = viewport.clientWidth || 960;
    var boxH = viewport.clientHeight || 540;
    var k = Math.min(boxW / natW, boxH / natH) * dpr * 1.1;
    var pw = Math.round(natW * k);
    var ph = Math.round(natH * k);
    if (pw >= natW || ph >= natH) return null; // 原图本来就没比预览大，直接解原图
    return { w: pw, h: ph, quality: "high" };
  }

  /* ---------------------------------------------------------------- 缓存 -- */

  function cacheKey(item, kind) { return item.id + "|" + kind; }

  function cacheGet(item, kind) {
    var e = state.cache.get(cacheKey(item, kind));
    if (e) e.at = performance.now();
    return e || null;
  }

  function cachePut(item, kind, data) {
    if (!data || !data.ok || !data.bitmap) return null;
    var key = cacheKey(item, kind);
    var old = state.cache.get(key);
    if (old && old.bitmap) {
      state.cacheBytes -= old.bytes;
      old.bitmap.close();
    }
    var entry = {
      bitmap: data.bitmap,
      w: data.bitmap.width,
      h: data.bitmap.height,
      bytes: data.bitmap.width * data.bitmap.height * 4,
      at: performance.now()
    };
    state.cache.set(key, entry);
    state.cacheBytes += entry.bytes;
    evict();
    updateMeters();
    return entry;
  }

  function dropEntry(key) {
    var entry = state.cache.get(key);
    if (!entry) return;
    state.cache.delete(key);
    state.cacheBytes -= entry.bytes;
    if (entry.bitmap) entry.bitmap.close();
  }

  function evict() {
    while (state.cacheBytes > CACHE_BUDGET && state.cache.size > 1) {
      var currentId = state.items[state.index] ? state.items[state.index].id + "|" : null;
      var oldestKey = null;
      var oldest = Infinity;
      state.cache.forEach(function (e, k) {
        if (currentId && k.indexOf(currentId) === 0) return; // 别把正在看的图淘汰掉
        if (e.at < oldest) { oldest = e.at; oldestKey = k; }
      });
      if (!oldestKey) break;
      dropEntry(oldestKey);
    }
  }

  function setFrame(entry, kind, owned) {
    if (state.frame && state.frame.owned && state.frame.entry !== entry) {
      state.frame.entry.bitmap.close();
    }
    state.frame = entry ? { entry: entry, kind: kind, owned: !!owned } : null;
  }

  /* ---------------------------------------------------------------- 绘制 -- */

  function resizeCanvas() {
    var dpr = deviceScale();
    var w = Math.max(1, Math.round(viewport.clientWidth * dpr));
    var h = Math.max(1, Math.round(viewport.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
      return true;
    }
    return false;
  }

  function fitView(item) {
    var fit = Math.min(canvas.width / item.w, canvas.height / item.h);
    state.view.fit = fit;
    state.view.scale = fit;
    state.view.tx = (canvas.width - item.w * fit) / 2;
    state.view.ty = (canvas.height - item.h * fit) / 2;
    state.view.mode = "fit";
  }

  function clampPan(item) {
    var cw = canvas.width;
    var ch = canvas.height;
    var iw = item.w * state.view.scale;
    var ih = item.h * state.view.scale;
    state.view.tx = iw <= cw ? (cw - iw) / 2 : clamp(state.view.tx, cw - iw, 0);
    state.view.ty = ih <= ch ? (ch - ih) / 2 : clamp(state.view.ty, ch - ih, 0);
  }

  function clearCanvas() {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#0a0810";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    viewport.classList.remove("has-frame");
  }

  function draw() {
    var item = state.items[state.index];
    clearCanvas();
    if (!item || !state.frame) return;
    clampPan(item);
    var zoom = state.view.scale / deviceScale();
    // 超过 1 个图像像素对 1 个物理像素时切最近邻 —— 但只有全分辨率纹理才够硬，
    // 预览纹理放大时仍然保持平滑，免得画面糊成马赛克。
    ctx.imageSmoothingEnabled = !(zoom >= 1 && state.frame.kind === "full");
    ctx.imageSmoothingQuality = "high";
    ctx.setTransform(state.view.scale, 0, 0, state.view.scale, state.view.tx, state.view.ty);
    // 关键：把位图映射到 item.w × item.h 这个「源图像素」矩形上。
    // 预览位图和全分辨率位图像素数不同，但几何一样 —— 这正是切换纹理时画面不跳的原因。
    ctx.drawImage(state.frame.entry.bitmap, 0, 0, item.w, item.h);
    viewport.classList.add("has-frame");
    updateZoomLabel();
  }

  function updateZoomLabel() {
    var pct = (state.view.scale / deviceScale()) * 100;
    var text;
    if (state.view.mode === "fit") text = "适应窗口 · " + Math.round(pct) + "%";
    else if (Math.abs(pct - 100) < 1.5) text = "100% · 像素对齐";
    else text = Math.round(pct) + "%";
    ui.zoom.textContent = text;
  }

  /* ------------------------------------------------------------ 仪表盘 -- */

  function updateMeters() {
    ui.hits.textContent = String(state.hits);
    ui.misses.textContent = String(state.misses);
    ui.avg.textContent = state.run.n ? (state.run.sum / state.run.n).toFixed(1) + " ms" : "--";
    ui.bytes.textContent = formatBytes(state.cacheBytes);
  }

  function record(ms, hit) {
    ms = Math.max(ms, 0.05);
    state.latencies.push({ ms: ms, hit: hit });
    if (state.latencies.length > HISTORY) state.latencies.shift();
    state.run.n += 1;
    state.run.sum += ms;
    ui.latencyValue.textContent = ms < 1 ? ms.toFixed(2) : ms.toFixed(1);
    ui.latency.classList.toggle("is-hit", hit);
    updateMeters();
    drawSpark();
  }

  function setBadge(text, kind) {
    ui.badge.textContent = text;
    ui.badge.classList.toggle("is-miss", kind === "miss");
    ui.badge.classList.toggle("is-cancel", kind === "cancel");
  }

  function drawSpark() {
    if (!spark) return;
    var dpr = deviceScale();
    var w = spark.clientWidth || 320;
    var h = spark.clientHeight || 62;
    if (spark.width !== Math.round(w * dpr) || spark.height !== Math.round(h * dpr)) {
      spark.width = Math.round(w * dpr);
      spark.height = Math.round(h * dpr);
    }
    var g = spark.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    g.fillStyle = "rgba(255,255,255,.07)";
    g.fillRect(0, h - 6, w, 1);
    var data = state.latencies;
    if (!data.length) return;
    var slot = w / HISTORY;
    var max = 4;
    for (var i = 0; i < data.length; i++) max = Math.max(max, data[i].ms);
    for (var j = 0; j < data.length; j++) {
      var bh = Math.max(2, Math.sqrt(data[j].ms / max) * (h - 16));
      g.fillStyle = data[j].hit ? "rgba(127,224,163,.9)" : "rgba(247,178,75,.9)";
      g.fillRect(j * slot, h - 6 - bh, Math.max(2, slot - 1.5), bh);
    }
  }

  var pipeTimers = [];

  function clearPipe() {
    pipeTimers.forEach(clearTimeout);
    pipeTimers = [];
    Object.keys(ui.steps).forEach(function (k) { ui.steps[k].classList.remove("is-on"); });
  }

  function animatePipeline(hit, res) {
    clearPipe();
    var order = ["read", "probe", "decode", "exif", "resize", "present"];
    var timings = {
      read: res ? res.fetchMs : 0,
      probe: res ? res.probeMs : 0,
      decode: res ? res.decodeMs : 0,
      exif: 0,
      resize: 0,
      present: 0
    };
    Object.keys(timings).forEach(function (k) {
      if (ui.steps[k]) ui.steps[k].title = timings[k] ? timings[k].toFixed(2) + " ms" : "";
    });
    var seq = hit ? ["present"] : order;
    var step = REDUCED ? 0 : 46;
    seq.forEach(function (name, i) {
      pipeTimers.push(setTimeout(function () {
        if (ui.steps[name]) ui.steps[name].classList.add("is-on");
      }, i * step));
    });
  }

  /* ---------------------------------------------------------------- 导航 -- */

  function updateChrome(item, index) {
    ui.path.textContent = item.local ? item.name : "assets/demo/" + item.name;
    ui.count.textContent = (index + 1) + " / " + state.items.length;
    ui.format.textContent = item.format;
    ui.dims.textContent = item.w + " × " + item.h;
    ui.title.textContent = item.title;
  }

  function highlightThumb() {
    state.items.forEach(function (it, i) {
      if (!it.thumbEl) return;
      var on = i === state.index;
      it.thumbEl.classList.toggle("is-current", on);
    });
    // 手动滚动缩略图条，而不是 scrollIntoView —— 后者会连带滚动所有可滚动祖先，
    // 把整个 Hero（overflow:hidden 的盒子也能被程序化滚动）推到左边去。
    var current = state.items[state.index];
    if (current && current.thumbEl && strip.scrollTo) {
      var target = current.thumbEl.offsetLeft - (strip.clientWidth - current.thumbEl.offsetWidth) / 2;
      strip.scrollTo({ left: Math.max(0, target), behavior: REDUCED ? "auto" : "smooth" });
    }
  }

  function cancelStale(seq) {
    var dropped = 0;
    queue = queue.filter(function (job) {
      if (job.priority === 0 && job.seq < seq) {
        job.cancelled = true;
        pending.delete(job.key);
        dropped++;
        return false;
      }
      return true;
    });
    slots.forEach(function (slot) {
      var job = slot.job;
      if (job && job.priority === 0 && job.seq < seq) { job.cancelled = true; dropped++; }
    });
    if (dropped) {
      state.cancels += dropped;
      setBadge("取消 " + dropped + " 个过期请求", "cancel");
    }
  }

  function ensureFull(item) {
    if (state.mode === "naive") return;
    if (!item || item.w * item.h < 1) return;
    if (state.view.scale / deviceScale() < 0.98) return;
    if (cacheGet(item, "full") || pending.has(cacheKey(item, "full"))) return;
    // 放大到 100% 以上才需要全分辨率：后台补一张，几何不变所以画面不会跳。
    setBadge("后台补全分辨率…", "miss");
    request(item, "full", { priority: 1, seq: state.seq }).promise.then(function (res) {
      if (!res || !res.ok || res.cancelled) return;
      var entry = cachePut(item, "full", res);
      if (state.items[state.index] === item) {
        setFrame(entry, "full", false);
        draw();
        setBadge("全分辨率纹理 " + res.width + "×" + res.height, "hit");
        animatePipeline(false, res);
      }
    });
  }

  function prefetch(center) {
    if (state.mode === "naive") return;
    var n = state.items.length;
    [1, -1, 2, -2].forEach(function (delta, i) {
      var item = state.items[((center + delta) % n + n) % n];
      if (!item || !item.w) return;
      if (cacheGet(item, "preview") || pending.has(cacheKey(item, "preview"))) return;
      request(item, "preview", { priority: 1 + i, seq: state.seq }).promise.then(function (res) {
        if (!res || !res.ok || res.cancelled) return;
        cachePut(item, "preview", res);
      });
    });
  }

  function show(index, opts) {
    opts = opts || {};
    var n = state.items.length;
    if (!n) return;
    index = ((index % n) + n) % n;
    var item = state.items[index];
    if (!item || !item.w || !item.h) return;
    if (state.index === index && !opts.force) return;

    var started = performance.now();
    state.index = index;
    state.seq += 1;
    var seq = state.seq;
    var naive = state.mode === "naive";

    updateChrome(item, index);
    highlightThumb();
    cancelStale(seq);
    resizeCanvas();
    fitView(item);
    clearPipe();
    viewport.classList.toggle("is-naive", naive);

    var hit = naive ? null : cacheGet(item, "preview");
    if (hit) {
      state.hits += 1;
      ui.spinner.hidden = true;
      setFrame(hit, "preview", false);
      draw();
      record(performance.now() - started, true);
      setBadge("缓存命中 · 零解码", "hit");
      animatePipeline(true, null);
      prefetch(index);
      ensureFull(item);
      return;
    }

    state.misses += 1;
    ui.spinner.hidden = false;
    var placeholder = naive ? null : cacheGet(item, "thumb");
    if (placeholder) {
      setFrame(placeholder, "thumb", false);
      draw();
    } else {
      setFrame(null);
      clearCanvas();
    }
    setBadge(naive ? "每次都从头解原图…" : "正在解码…", "miss");

    // 朴素管线加载的是原图（不缩放），这正是慢的根源。
    var kind = naive ? "full" : "preview";
    request(item, kind, { priority: 0, seq: seq }).promise.then(function (res) {
      if (!res) return;
      if (res.cancelled || state.seq !== seq) return;
      if (!res.ok) {
        ui.spinner.hidden = true;
        setBadge("解码失败：" + (res.error || "未知错误"), "miss");
        return;
      }
      var entry = naive ? { bitmap: res.bitmap, w: res.width, h: res.height, bytes: res.width * res.height * 4, at: performance.now() } : cachePut(item, kind, res);
      ui.spinner.hidden = true;
      setFrame(entry, kind, naive);
      draw();
      record(performance.now() - started, false);
      setBadge((naive ? "解了整张原图 " : "现解码 ") + res.decodeMs.toFixed(1) + " ms · " + res.width + "×" + res.height, "miss");
      animatePipeline(false, res);
      if (!naive) { prefetch(index); ensureFull(item); }
    });
  }

  function next(delta) {
    stopAutoplay();
    if (state.index < 0) return;
    show(state.index + delta);
  }

  /* ------------------------------------------------------------ 缩放平移 -- */

  function pointerPos(ev) {
    var rect = viewport.getBoundingClientRect();
    var k = canvas.width / Math.max(rect.width, 1);
    return { x: (ev.clientX - rect.left) * k, y: (ev.clientY - rect.top) * k };
  }

  function zoomAt(px, py, factor) {
    var item = state.items[state.index];
    if (!item) return;
    var nextScale = clamp(state.view.scale * factor, state.view.fit * 0.2, state.view.fit * 64);
    var ratio = nextScale / state.view.scale;
    state.view.tx = px - (px - state.view.tx) * ratio;
    state.view.ty = py - (py - state.view.ty) * ratio;
    state.view.scale = nextScale;
    state.view.mode = "free";
    draw();
    ensureFull(item);
  }

  viewport.addEventListener("wheel", function (ev) {
    if (state.index < 0) return;
    // 已经在「适应窗口」还要缩小是没有意义的 —— 这时候把滚轮还给页面，
    // 否则鼠标停在演示台上就滚不到下面的正文了。
    if (ev.deltaY > 0 && state.view.scale <= state.view.fit * 1.001) return;
    ev.preventDefault();
    stopAutoplay();
    var p = pointerPos(ev);
    zoomAt(p.x, p.y, Math.exp(-ev.deltaY * (ev.deltaMode === 1 ? 0.05 : 0.0016)));
  }, { passive: false });

  viewport.addEventListener("pointerdown", function (ev) {
    if (state.index < 0) return;
    stopAutoplay();
    viewport.focus({ preventScroll: true });
    var rect = viewport.getBoundingClientRect();
    state.drag = {
      x: ev.clientX,
      y: ev.clientY,
      tx: state.view.tx,
      ty: state.view.ty,
      k: canvas.width / Math.max(rect.width, 1)
    };
    try { viewport.setPointerCapture(ev.pointerId); } catch (e) { /* ignore */ }
  });

  viewport.addEventListener("pointermove", function (ev) {
    if (!state.drag) return;
    var item = state.items[state.index];
    if (!item) return;
    state.view.tx = state.drag.tx + (ev.clientX - state.drag.x) * state.drag.k;
    state.view.ty = state.drag.ty + (ev.clientY - state.drag.y) * state.drag.k;
    state.view.mode = "free";
    draw();
  });

  function endDrag(ev) {
    if (!state.drag) return;
    state.drag = null;
    try { viewport.releasePointerCapture(ev.pointerId); } catch (e) { /* ignore */ }
  }

  viewport.addEventListener("pointerup", endDrag);
  viewport.addEventListener("pointercancel", endDrag);

  function togglePixelZoom() {
    var item = state.items[state.index];
    if (!item) return;
    var zoom = state.view.scale / deviceScale();
    if (Math.abs(zoom - 1) < 0.02) {
      fitView(item);
    } else {
      state.view.scale = deviceScale();
      state.view.tx = (canvas.width - item.w * state.view.scale) / 2;
      state.view.ty = (canvas.height - item.h * state.view.scale) / 2;
      state.view.mode = "free";
      ensureFull(item);
    }
    draw();
  }

  viewport.addEventListener("dblclick", function () {
    stopAutoplay();
    togglePixelZoom();
  });

  window.addEventListener("keydown", function (ev) {
    if (state.index < 0) return;
    var tag = (ev.target && ev.target.tagName) || "";
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    if (!(root.contains(document.activeElement) || viewport.matches(":hover"))) return;
    var item = state.items[state.index];
    var handled = true;
    if (ev.key === "ArrowRight" || ev.key === "ArrowDown" || ev.key === " ") next(1);
    else if (ev.key === "ArrowLeft" || ev.key === "ArrowUp") next(-1);
    else if (ev.key === "Home") { stopAutoplay(); show(0); }
    else if (ev.key === "End") { stopAutoplay(); show(state.items.length - 1); }
    else if (ev.key === "1") { togglePixelZoom(); }
    else if (ev.key === "0" || (ev.key.length === 1 && ev.key.toLowerCase() === "f")) {
      if (item) { fitView(item); draw(); }
    } else handled = false;
    if (handled) { ev.preventDefault(); stopAutoplay(); }
  });

  /* ------------------------------------------------------------ 图集装配 -- */

  function buildThumb(item) {
    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "mx-thumb";
    btn.setAttribute("aria-label", item.title);
    var cv = document.createElement("canvas");
    var aspect = item.w && item.h ? item.h / item.w : 2 / 3;
    cv.width = 180;
    cv.height = Math.max(1, Math.round(180 * aspect));
    btn.appendChild(cv);
    var tag = document.createElement("span");
    tag.className = "mx-thumb__fmt";
    tag.textContent = item.format;
    btn.appendChild(tag);
    btn.addEventListener("click", function () {
      stopAutoplay();
      show(state.items.indexOf(item));
    });
    strip.appendChild(btn);
    item.thumbEl = btn;
    item.thumbCanvas = cv;
  }

  function loadThumb(item) {
    return request(item, "thumb", { priority: 2, seq: -1 }).promise.then(function (res) {
      if (!res || !res.ok || !res.bitmap || res.cancelled) {
        if (res && res.bitmap) res.bitmap.close();
        return;
      }
      // 缩略图留在缓存里还有一个用处：主图还没解完时先拿它撑住画面，
      // 这就是「先预览、后全分辨率」里最便宜的那一级。
      var entry = cachePut(item, "thumb", res);
      if (!entry) return;
      var cv = item.thumbCanvas;
      var g = cv.getContext("2d");
      var scale = Math.max(cv.width / entry.w, cv.height / entry.h);
      var dw = entry.w * scale;
      var dh = entry.h * scale;
      g.drawImage(entry.bitmap, (cv.width - dw) / 2, (cv.height - dh) / 2, dw, dh);
      item.thumbEl.classList.add("is-ready");
    });
  }

  function addCorpus() {
    (window.MV_CORPUS || []).forEach(function (rec) {
      state.items.push({
        id: state.nextId++,
        name: rec.file,
        url: BASE + rec.file,
        title: rec.title,
        format: rec.format,
        w: rec.w,
        h: rec.h,
        bytes: rec.bytes,
        local: false
      });
    });
  }

  function probe(item) {
    return request(item, "probe", { priority: 0, seq: state.seq }).promise.then(function (res) {
      if (res && res.ok && res.width) { item.w = res.width; item.h = res.height; }
      if (res && res.bitmap) res.bitmap.close();
      return item;
    });
  }

  function addLocalFiles(files) {
    var added = [];
    Array.prototype.forEach.call(files, function (file) {
      if (!/^image\//.test(file.type) && !/\.(jpe?g|png|webp|gif|bmp|avif|tiff?)$/i.test(file.name)) return;
      var item = {
        id: state.nextId++,
        name: file.name,
        url: URL.createObjectURL(file),
        title: file.name.replace(/\.[^.]+$/, ""),
        format: extension(file.name),
        w: 0,
        h: 0,
        bytes: file.size,
        local: true
      };
      state.items.push(item);
      buildThumb(item);
      added.push(item);
    });
    if (!added.length) {
      setBadge("没找到可识别的图片文件", "cancel");
      return Promise.resolve();
    }
    setBadge("正在读取文件头…", "miss");
    return Promise.all(added.map(probe)).then(function () {
      var ready = added.filter(function (it) { return it.w && it.h; });
      ready.forEach(loadThumb);
      if (!ready.length) {
        setBadge("这几张图浏览器解不了", "miss");
        return;
      }
      show(state.items.indexOf(ready[0]), { force: true });
      setBadge("已加入 " + added.length + " 张本地照片", "hit");
    });
  }

  var dragDepth = 0;
  ["dragenter", "dragover"].forEach(function (name) {
    root.addEventListener(name, function (ev) {
      if (!ev.dataTransfer || !ev.dataTransfer.types || ev.dataTransfer.types.indexOf("Files") < 0) return;
      ev.preventDefault();
      if (name === "dragenter") dragDepth++;
      ui.drop.hidden = false;
    });
  });
  ["dragleave", "drop"].forEach(function (name) {
    root.addEventListener(name, function (ev) {
      if (name === "dragleave") {
        dragDepth = Math.max(0, dragDepth - 1);
        if (dragDepth > 0) return;
      } else {
        dragDepth = 0;
        ev.preventDefault();
        if (ev.dataTransfer && ev.dataTransfer.files && ev.dataTransfer.files.length) {
          stopAutoplay();
          addLocalFiles(ev.dataTransfer.files);
        }
      }
      ui.drop.hidden = true;
    });
  });

  if (ui.open) {
    ui.open.addEventListener("change", function () {
      if (ui.open.files && ui.open.files.length) {
        stopAutoplay();
        addLocalFiles(ui.open.files);
        ui.open.value = "";
      }
    });
  }

  /* ------------------------------------------------------------ 模式切换 -- */

  var PREV_DESIRE = {
    fast: "闪电管线：预取 + 字节预算缓存",
    naive: "朴素管线：无预取、无缓存、每次都解原图"
  };

  function setMode(mode) {
    if (state.mode === mode) return;
    state.mode = mode;
    state.hits = 0;
    state.misses = 0;
    state.cancels = 0;
    state.latencies = [];
    state.run = { n: 0, sum: 0 };
    queue.forEach(function (job) { job.cancelled = true; pending.delete(job.key); });
    queue = [];
    slots.forEach(function (slot) { if (slot.job) slot.job.cancelled = true; });
    // 先丢掉对当前帧的引用，再清缓存 —— 否则会留下一个指向已 close 位图的悬空引用。
    setFrame(null);
    // 清掉预览/全分辨率缓存（缩略图留着，它属于界面的一部分）。
    Array.from(state.cache.keys()).forEach(function (key) {
      if (key.indexOf("|thumb") < 0) dropEntry(key);
    });
    ui.toggles.forEach(function (btn) {
      btn.classList.toggle("is-active", btn.getAttribute("data-mv-mode") === mode);
    });
    clearPipe();
    drawSpark();
    updateMeters();
    setBadge(PREV_DESIRE[mode], mode === "naive" ? "miss" : "hit");
    show(state.index, { force: true });
  }

  ui.toggles.forEach(function (btn) {
    btn.addEventListener("click", function () {
      stopAutoplay();
      setMode(btn.getAttribute("data-mv-mode"));
    });
  });

  /* ------------------------------------------------------------ 数字滚动 -- */

  function countUp() {
    if (REDUCED) return;
    ui.stats.forEach(function (el) {
      var target = parseFloat(el.getAttribute("data-count"));
      if (!isFinite(target)) return;
      var dec = parseInt(el.getAttribute("data-decimals") || "0", 10);
      var suffix = el.getAttribute("data-suffix") || "";
      var t0 = performance.now();
      var dur = 1100;
      function tick(now) {
        var p = Math.min(1, (now - t0) / dur);
        el.textContent = (target * (1 - Math.pow(1 - p, 3))).toFixed(dec) + suffix;
        if (p < 1) requestAnimationFrame(tick);
      }
      requestAnimationFrame(tick);
    });
  }

  /* ------------------------------------------------------------ 自动巡览 -- */

  var autoplayTimer = null;
  var wantAutoplay = false;

  function stopAutoplay() {
    if (autoplayTimer) { clearInterval(autoplayTimer); autoplayTimer = null; }
  }

  function startAutoplay() {
    if (autoplayTimer || REDUCED || document.hidden || !state.ready) return;
    var stops = Math.min(8, state.items.length);
    var step = 0;
    autoplayTimer = setInterval(function () {
      step += 1;
      if (step >= stops) {
        stopAutoplay();
        setBadge("缓存已热 · 现在随便翻", "hit");
        return;
      }
      show(state.index + 1);
    }, 460);
  }

  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting && entry.intersectionRatio > 0.4) {
          io.disconnect();
          wantAutoplay = true;
          startAutoplay();
        }
      });
    }, { threshold: [0.4] });
    io.observe(root);
  }

  var resizeTimer = null;
  window.addEventListener("resize", function () {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(function () {
      resizeCanvas();
      var item = state.items[state.index];
      if (!item) return;
      if (state.view.mode === "fit") fitView(item);
      draw();
      drawSpark();
    }, 120);
  });

  if ("ResizeObserver" in window) {
    new ResizeObserver(function () {
      if (!state.ready) return;
      resizeCanvas();
      var item = state.items[state.index];
      if (!item) return;
      if (state.view.mode === "fit") fitView(item);
      // 改 canvas.width 会清空画布，所以这里必须重画，不管当前是不是适应窗口。
      draw();
    }).observe(viewport);
  }

  /* ---------------------------------------------------------------- 启动 -- */

  addCorpus();
  state.items.forEach(buildThumb);
  updateMeters();
  drawSpark();

  // 先把缩略图条铺出来（相当于 M2 的缩略图条），再放第一张主图。
  Promise.all(state.items.map(loadThumb)).then(function () {
    if (!state.items.length) {
      ui.empty.textContent = "没有可用的演示图片。";
      return;
    }
    resizeCanvas();
    state.ready = true;
    show(0, { force: true });
    countUp();
    if (wantAutoplay) startAutoplay();
  });
})();
