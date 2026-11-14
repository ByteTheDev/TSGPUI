const ID_SYMBOL = Symbol('rendrix.id');
// ---------------------------------------------------------------------------------------------
// Events: Event hierarchy, EventTarget, dispatch with capture/bubble phases.
// ---------------------------------------------------------------------------------------------

// Listener state is keyed by node id (not by the JS wrapper): wrappers are cached weakly, so state
// stored on a wrapper could vanish with it. Non-node targets (window, AbortSignal...) key by object.
const NODE_STATE = new Map();     // id -> { listeners: Map, handlers: Map }
const OBJ_STATE = new WeakMap();  // object -> { listeners: Map, handlers: Map }
function stateOf(target, create) {
  const id = target[ID_SYMBOL];
  const store = id === undefined ? OBJ_STATE : NODE_STATE;
  const key = id === undefined ? target : id;
  let st = store.get(key);
  if (!st && create) { st = { listeners: new Map(), handlers: new Map() }; store.set(key, st); }
  return st;
}
const forgetListeners = id => NODE_STATE.delete(id);

class DOMException extends Error {
  constructor(message = '', name = 'Error') {
    super(message);
    Object.defineProperty(this, 'name', { value: name, configurable: true });
    this.code = { IndexSizeError: 1, HierarchyRequestError: 3, NotFoundError: 8, NotSupportedError: 9, InvalidStateError: 11, SyntaxError: 12, AbortError: 20, TimeoutError: 23, NetworkError: 19 }[name] || 0;
  }
}

class Event {
  static NONE = 0; static CAPTURING_PHASE = 1; static AT_TARGET = 2; static BUBBLING_PHASE = 3;
  #stop = false; #stopImmediate = false; #canceled = false;
  constructor(type, init = {}) {
    this.type = String(type);
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.composed = !!init.composed;
    this.target = null;
    this.currentTarget = null;
    this.eventPhase = 0;
    this.isTrusted = false;
    this.timeStamp = global.performance ? global.performance.now() : 0;
    this._path = null;
    this._inPassive = false;
  }
  get defaultPrevented() { return this.#canceled; }
  get srcElement() { return this.target; }
  get cancelBubble() { return this.#stop; }
  set cancelBubble(v) { if (v) this.#stop = true; }
  get returnValue() { return !this.#canceled; }
  preventDefault() { if (this.cancelable && !this._inPassive) this.#canceled = true; }
  stopPropagation() { this.#stop = true; }
  stopImmediatePropagation() { this.#stop = true; this.#stopImmediate = true; }
  composedPath() { return this._path ? this._path.slice() : []; }
  get _stopped() { return this.#stop; }
  get _stoppedImmediate() { return this.#stopImmediate; }
}

class CustomEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.detail = init.detail === undefined ? null : init.detail; }
}
class UIEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.detail = init.detail || 0; this.view = init.view || null; }
}
class FocusEvent extends UIEvent {
  constructor(type, init = {}) { super(type, init); this.relatedTarget = init.relatedTarget || null; }
}
class MouseEvent extends UIEvent {
  constructor(type, init = {}) {
    super(type, init);
    this.screenX = init.screenX || 0; this.screenY = init.screenY || 0;
    this.clientX = init.clientX || 0; this.clientY = init.clientY || 0;
    this.pageX = init.pageX !== undefined ? init.pageX : this.clientX; this.pageY = init.pageY !== undefined ? init.pageY : this.clientY;
    this.x = this.clientX; this.y = this.clientY;
    this.offsetX = init.offsetX || 0; this.offsetY = init.offsetY || 0;
    this.movementX = init.movementX || 0; this.movementY = init.movementY || 0;
    this.button = init.button || 0; this.buttons = init.buttons || 0;
    this.ctrlKey = !!init.ctrlKey; this.shiftKey = !!init.shiftKey; this.altKey = !!init.altKey; this.metaKey = !!init.metaKey;
    this.relatedTarget = init.relatedTarget || null;
  }
  getModifierState(k) { return { Control: this.ctrlKey, Shift: this.shiftKey, Alt: this.altKey, Meta: this.metaKey }[k] || false; }
}
class PointerEvent extends MouseEvent {
  constructor(type, init = {}) {
    super(type, init);
    this.pointerId = init.pointerId === undefined ? 1 : init.pointerId;
    this.pointerType = init.pointerType || 'mouse';
    this.isPrimary = init.isPrimary === undefined ? true : init.isPrimary;
    this.width = init.width || 1; this.height = init.height || 1; this.pressure = init.pressure || 0;
  }
}
class WheelEvent extends MouseEvent {
  static DOM_DELTA_PIXEL = 0; static DOM_DELTA_LINE = 1; static DOM_DELTA_PAGE = 2;
  constructor(type, init = {}) {
    super(type, init);
    this.deltaX = init.deltaX || 0; this.deltaY = init.deltaY || 0; this.deltaZ = init.deltaZ || 0; this.deltaMode = init.deltaMode || 0;
  }
}
class KeyboardEvent extends UIEvent {
  constructor(type, init = {}) {
    super(type, init);
    this.key = init.key === undefined ? '' : init.key; this.code = init.code || '';
    this.repeat = !!init.repeat; this.location = init.location || 0; this.isComposing = !!init.isComposing;
    this.ctrlKey = !!init.ctrlKey; this.shiftKey = !!init.shiftKey; this.altKey = !!init.altKey; this.metaKey = !!init.metaKey;
  }
  getModifierState(k) { return { Control: this.ctrlKey, Shift: this.shiftKey, Alt: this.altKey, Meta: this.metaKey }[k] || false; }
}
class InputEvent extends UIEvent {
  constructor(type, init = {}) { super(type, init); this.data = init.data === undefined ? null : init.data; this.inputType = init.inputType || ''; this.isComposing = !!init.isComposing; }
}
class SubmitEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.submitter = init.submitter || null; }
}
class ErrorEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.message = init.message || ''; this.filename = init.filename || ''; this.lineno = init.lineno || 0; this.colno = init.colno || 0; this.error = init.error; }
}
class MessageEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.data = init.data; this.origin = init.origin || ''; this.source = init.source || null; }
}
class PromiseRejectionEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.promise = init.promise; this.reason = init.reason; }
}
class ProgressEvent extends Event {
  constructor(type, init = {}) { super(type, init); this.loaded = init.loaded || 0; this.total = init.total || 0; this.lengthComputable = !!init.lengthComputable; }
}

// QuickJS stacks omit the "Name: message" header; add it.
function errorText(err) {
  if (err && err.stack) {
    const head = err.name + ': ' + err.message;
    const st = String(err.stack);
    return st.startsWith(head) ? st : head + '\n' + st;
  }
  return String(err);
}
function reportError(err) {
  const msg = errorText(err);
  N('console', 'error', 'Uncaught ' + msg);
  try {
    const ev = new ErrorEvent('error', { message: err && err.message ? err.message : String(err), error: err, cancelable: true });
    windowTarget._rawDispatch(ev);
  } catch (_) { /* never throw from the error reporter */ }
}

class EventTarget {
  addEventListener(type, callback, options) {
    if (callback == null) return;
    const opts = typeof options === 'boolean' ? { capture: options } : (options || {});
    const map = stateOf(this, true).listeners;
    let list = map.get(type);
    if (!list) { list = []; map.set(type, list); }
    const capture = !!opts.capture;
    if (list.some(l => l.callback === callback && l.capture === capture)) return;
    const l = { callback, capture, once: !!opts.once, passive: !!opts.passive, removed: false };
    list.push(l);
    if (opts.signal) {
      if (opts.signal.aborted) { this.removeEventListener(type, callback, { capture }); return; }
      opts.signal.addEventListener('abort', () => this.removeEventListener(type, callback, { capture }), { once: true });
    }
  }
  removeEventListener(type, callback, options) {
    const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
    const st = stateOf(this, false);
    const list = st && st.listeners.get(type);
    if (!list) return;
    const i = list.findIndex(l => l.callback === callback && l.capture === capture);
    if (i >= 0) { list[i].removed = true; list.splice(i, 1); }
  }
  dispatchEvent(event) {
    if (!(event instanceof Event)) throw new TypeError('dispatchEvent requires an Event');
    return this._rawDispatch(event);
  }
  _eventParent() { return null; }
  _rawDispatch(event) {
    // Build the propagation path: target → … → root (document, then window).
    const path = [];
    for (let t = this; t; t = t._eventParent()) path.push(t);
    event.target = this;
    event._path = path;
    // Capture phase (root → target's parent).
    event.eventPhase = 1;
    for (let i = path.length - 1; i > 0 && !event._stopped; i--) invoke(path[i], event, true);
    // Target phase: capturing listeners first, then non-capturing.
    if (!event._stopped) { event.eventPhase = 2; invoke(path[0], event, true); }
    if (!event._stopped) invoke(path[0], event, false);
    // Bubble phase.
    if (event.bubbles) {
      event.eventPhase = 3;
      for (let i = 1; i < path.length && !event._stopped; i++) invoke(path[i], event, false);
    }
    event.eventPhase = 0;
    event.currentTarget = null;
    return !event.defaultPrevented;
  }
}

function invoke(target, event, capture) {
  const st = stateOf(target, false);
  const list = st && st.listeners.get(event.type);
  if (!list || list.length === 0) return;
  event.currentTarget = target;
  for (const l of list.slice()) {
    if (l.removed || l.capture !== capture) continue;
    if (l.once) target.removeEventListener(event.type, l.callback, { capture: l.capture });
    event._inPassive = l.passive;
    try {
      if (typeof l.callback === 'function') l.callback.call(target, event);
      else if (l.callback && typeof l.callback.handleEvent === 'function') l.callback.handleEvent(event);
    } catch (err) {
      reportError(err);
    }
    event._inPassive = false;
    if (event._stoppedImmediate) break;
  }
}

// `onclick`-style handler properties: a single listener slot per type, kept in registration order.
const ON_EVENTS = ['click', 'dblclick', 'mousedown', 'mouseup', 'mousemove', 'mouseover', 'mouseout', 'mouseenter', 'mouseleave', 'contextmenu',
  'pointerdown', 'pointerup', 'pointermove', 'pointerover', 'pointerout', 'pointerenter', 'pointerleave', 'pointercancel',
  'keydown', 'keyup', 'keypress', 'input', 'change', 'submit', 'reset', 'focus', 'blur', 'focusin', 'focusout', 'wheel', 'scroll',
  'load', 'error', 'abort', 'resize', 'select', 'invalid', 'drag', 'drop', 'touchstart', 'touchend', 'touchmove', 'animationend', 'transitionend',
  'beforeunload', 'unload', 'hashchange', 'popstate', 'message', 'visibilitychange', 'readystatechange'];
for (const type of ON_EVENTS) {
  Object.defineProperty(EventTarget.prototype, 'on' + type, {
    configurable: true,
    get() { const st = stateOf(this, false); const h = st && st.handlers.get(type); return h ? h.fn : null; },
    set(fn) {
      const m = stateOf(this, true).handlers;
      let h = m.get(type);
      if (!h) {
        h = { fn: null, listener: null };
        h.listener = function (ev) { if (typeof h.fn === 'function') return h.fn.call(this, ev); };
        m.set(type, h);
        this.addEventListener(type, h.listener);
      }
      h.fn = typeof fn === 'function' ? fn : null;
    },
  });
}
