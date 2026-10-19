// Line-synced lyrics of the playing track. The phone sends the lines as a list at path "lyrics"
// (row id = start time in ms, empty when unsynced; title = the line). The page follows the current
// line; up/down focus other lines, select seeks to the focused line, back returns to following
// (and closes the page when already following).
#include "hocket.h"

#define MAX_LINES 160
// The phone cuts a row's title to 47 bytes (PebbleProtocol.encodeRows), so a line never needs more.
#define LINE_LEN 48
#define LYRICS_PREFETCH 12
#define RETRY_MS 5000  // re-request a page the phone has not answered in 5 s
// Wake just after the next line starts, so the time it is drawn at is already past its start.
#define FOLLOW_SLACK_MS 20

typedef struct {
  int32_t start_ms;  // -1 when unsynced
  char text[LINE_LEN];
} Line;

static Window *s_window;
static Layer *s_layer;
static Line *s_lines;
static uint16_t s_count, s_total;
static uint16_t s_req_id;
static int16_t s_current = -1;  // the line playing now
static int16_t s_focus;         // the line in the middle
static bool s_manual;           // focus moved by the buttons
static bool s_loaded;
static AppTimer *s_follow_timer;  // the next line's start
static AppTimer *s_retry_timer;   // a page request the phone has not answered
static bool s_waiting;            // a page request is out
static char s_title[TEXT_LEN];

static void prv_request(uint16_t offset);

// The request or its reply got lost (not connected, the phone busy): ask again.
static void prv_retry(void *context) {
  s_retry_timer = NULL;
  if (s_waiting && s_lines) prv_request(s_count);
}

static void prv_request(uint16_t offset) {
  static uint16_t next = 0x8000;  // lists use ids below this
  if (next < 0x8000) next = 0x8000;
  s_req_id = next++;
  s_waiting = true;
  if (s_retry_timer) app_timer_reschedule(s_retry_timer, RETRY_MS);
  else s_retry_timer = app_timer_register(RETRY_MS, prv_retry, NULL);
  send_list_request(s_req_id, "lyrics", offset);
}

/** Ask for the next page only when the playhead or focus is near the last loaded line. */
static void prv_prefetch(void) {
  if (s_waiting || s_count >= s_total) return;
  int16_t need = s_focus;
  if (s_current > need) need = s_current;
  if (need < 0) need = 0;
  if (need + LYRICS_PREFETCH >= (int16_t)s_count) prv_request(s_count);
}

static int16_t prv_line_at(uint32_t pos) {
  int16_t current = -1;
  for (int i = 0; i < s_count; i++) {
    if (s_lines[i].start_ms < 0) return -1;
    if ((uint32_t)s_lines[i].start_ms <= pos) current = i;
    else break;
  }
  return current;
}

static void prv_follow(void *context);

// Moves to the line playing now, then sleeps until the next one starts. Nothing wakes the page
// while paused, before lyrics load or when they are unsynced: a Now message (play, pause, seek)
// comes in through lyrics_position_changed, a page through lyrics_handle_page.
static void prv_sync(void) {
  const uint32_t pos = now_position_ms();
  const int16_t line = prv_line_at(pos);
  if (line != s_current) {
    s_current = line;
    if (!s_manual) s_focus = line < 0 ? 0 : line;
    layer_mark_dirty(s_layer);
  }
  if (s_follow_timer) app_timer_cancel(s_follow_timer);
  s_follow_timer = NULL;
  const int16_t next = line + 1;
  if (!(g_now.flags & NowPlaying) || next >= s_count || s_lines[next].start_ms < 0) return;
  const uint32_t start = s_lines[next].start_ms;
  s_follow_timer = app_timer_register((start > pos ? start - pos : 0) + FOLLOW_SLACK_MS, prv_follow, NULL);
}

static void prv_follow(void *context) {
  s_follow_timer = NULL;
  if (!s_window || !s_lines) return;
  prv_sync();
  prv_prefetch();
}

void lyrics_position_changed(void) {
  if (!s_window || !s_lines) return;
  prv_sync();
  prv_prefetch();
}

void lyrics_handle_page(DictionaryIterator *iter) {
  Tuple *req = dict_find(iter, MESSAGE_KEY_ReqId);
  if (!req || req->value->uint16 != s_req_id || !s_lines) return;
  s_waiting = false;
  if (s_retry_timer) app_timer_cancel(s_retry_timer);
  s_retry_timer = NULL;
  Tuple *total = dict_find(iter, MESSAGE_KEY_Total);
  Tuple *offset = dict_find(iter, MESSAGE_KEY_Offset);
  Tuple *items = dict_find(iter, MESSAGE_KEY_Items);
  Tuple *title = dict_find(iter, MESSAGE_KEY_ListTitle);
  if (title && title->type == TUPLE_CSTRING) strncpy(s_title, title->value->cstring, sizeof(s_title) - 1);
  s_total = total ? total->value->uint16 : 0;
  if (s_total > MAX_LINES) s_total = MAX_LINES;
  uint16_t at = offset ? offset->value->uint16 : 0;
  if (at != s_count) {
    // Not the page that follows what we have: ask for that one again.
    prv_request(s_count);
    return;
  }
  if (items) {
    const uint8_t *p = items->value->data, *end = p + items->length;
    while (p < end && s_count < s_total) {
      p++;  // flags
      char field[3][LINE_LEN];
      bool whole = true;
      for (int f = 0; f < 3; f++) {
        const uint8_t *z = memchr(p, 0, end - p);
        if (!z) { whole = false; break; }
        size_t len = z - p;
        if (len >= LINE_LEN) len = LINE_LEN - 1;
        memcpy(field[f], p, len);
        field[f][len] = 0;
        p = z + 1;
      }
      // A row cut off mid-message: its fields were never written.
      if (!whole || p > end) break;
      Line *l = &s_lines[s_count++];
      l->start_ms = field[0][0] ? atoi(field[0]) : -1;
      strcpy(l->text, field[1]);
    }
  }
  s_loaded = true;
  s_current = -2;  // recompute
  prv_sync();
  prv_prefetch();
  layer_mark_dirty(s_layer);
}

#ifdef HOCKET_DEMO
// Invented placeholder lines so the emulator shows the page without a phone.
static void prv_demo(void) {
  static const char *const LINES[] = {
    "(instrumental)",
    "First line of the demo verse",
    "Second line, a little longer than the first",
    "Third line of the verse",
    "Fourth line ends the verse",
    "Placeholder chorus begins here",
    "A line that repeats in the chorus",
    "Another chorus line for testing",
    "The chorus closes on this line",
    "(instrumental)",
    "Second verse, first sample line",
    "Last line of the demo",
  };
  s_count = s_total = ARRAY_LENGTH(LINES);
  // Timed around the demo position, so the page opens mid-song.
  const int32_t start = (int32_t)now_position_ms() - 22000;
  for (int i = 0; i < s_count; i++) {
    s_lines[i].start_ms = start + i * 4000;
    strncpy(s_lines[i].text, LINES[i], LINE_LEN - 1);
    s_lines[i].text[LINE_LEN - 1] = 0;
  }
  s_loaded = true;
}
#endif

void lyrics_track_changed(void) {
  if (!s_window) return;
  s_count = s_total = 0;
  s_loaded = false;
  s_manual = false;
  s_focus = 0;
  s_current = -1;
#ifdef HOCKET_DEMO
  prv_demo();
  prv_sync();
#else
  prv_request(0);
#endif
  layer_mark_dirty(s_layer);
}

// The focused line sits in the middle; the others stack above and below it, all left-aligned.
static void prv_draw(Layer *layer, GContext *ctx) {
  const GRect b = layer_get_bounds(layer);
  // Left-aligned at Now Playing's text margin; on round, far enough in that the lines around the
  // middle (the widest part of the page) clear the bezel.
  const int16_t inset = PBL_IF_ROUND_ELSE(b.size.w * 17 / 100, b.size.h >= 200 ? 10 : 12);
  const int16_t w = b.size.w - 2 * inset;
  GFont focus_font = fonts_get_system_font(b.size.h >= 200 ? FONT_KEY_GOTHIC_28_BOLD : FONT_KEY_GOTHIC_24_BOLD);
  GFont other_font = fonts_get_system_font(b.size.h >= 200 ? FONT_KEY_GOTHIC_24 : FONT_KEY_GOTHIC_18);
  if (!s_loaded || !s_count) {
    graphics_context_set_text_color(ctx, GColorWhite);
    graphics_draw_text(ctx, s_loaded ? "No lyrics" : "Loading…", other_font,
                       GRect(inset, b.size.h / 2 - 15, w, 30), GTextOverflowModeTrailingEllipsis,
                       GTextAlignmentLeft, NULL);
    return;
  }
  const int16_t focus = s_focus < s_count ? s_focus : s_count - 1;
  GSize fs = graphics_text_layout_get_content_size(s_lines[focus].text, focus_font, GRect(0, 0, w, 200),
                                                   GTextOverflowModeWordWrap, GTextAlignmentLeft);
  int16_t top = b.size.h / 2 - fs.h / 2 - 4;
  // The focused line: white when it is the one playing, the accent while browsing.
  graphics_context_set_text_color(ctx, focus == s_current || !s_manual ? GColorWhite
                                                                     : PBL_IF_COLOR_ELSE(GColorChromeYellow, GColorWhite));
  graphics_draw_text(ctx, s_lines[focus].text, focus_font, GRect(inset, top, w, fs.h + 6),
                     GTextOverflowModeWordWrap, GTextAlignmentLeft, NULL);
  graphics_context_set_text_color(ctx, PBL_IF_COLOR_ELSE(GColorLightGray, GColorWhite));
  int16_t y = top + fs.h + 6;
  for (int i = focus + 1; i < s_count && y < b.size.h; i++) {
    GSize s = graphics_text_layout_get_content_size(s_lines[i].text, other_font, GRect(0, 0, w, 200),
                                                    GTextOverflowModeWordWrap, GTextAlignmentLeft);
    graphics_draw_text(ctx, s_lines[i].text, other_font, GRect(inset, y, w, s.h + 4),
                       GTextOverflowModeWordWrap, GTextAlignmentLeft, NULL);
    y += s.h + 4;
  }
  y = top;
  for (int i = focus - 1; i >= 0 && y > 0; i--) {
    GSize s = graphics_text_layout_get_content_size(s_lines[i].text, other_font, GRect(0, 0, w, 200),
                                                    GTextOverflowModeWordWrap, GTextAlignmentLeft);
    y -= s.h + 4;
    graphics_draw_text(ctx, s_lines[i].text, other_font, GRect(inset, y, w, s.h + 4),
                       GTextOverflowModeWordWrap, GTextAlignmentLeft, NULL);
  }
}

static void prv_move(int delta) {
  if (!s_count) return;
  int16_t next = s_focus + delta;
  if (next < 0) next = 0;
  if (next >= s_count) next = s_count - 1;
  s_focus = next;
  s_manual = s_focus != s_current;
  prv_prefetch();
  layer_mark_dirty(s_layer);
}

static void prv_up(ClickRecognizerRef r, void *ctx) { prv_move(-1); }
static void prv_down(ClickRecognizerRef r, void *ctx) { prv_move(1); }

static void prv_select(ClickRecognizerRef r, void *ctx) {
  if (!s_count || s_lines[s_focus].start_ms < 0) return;
  send_cmd(CmdSeek, s_lines[s_focus].start_ms, NULL, NULL);
  // Jump the local clock too, so the page follows from there straight away.
  g_now.position_ms = s_lines[s_focus].start_ms;
  time_ms(&g_now.position_at, &g_now.position_at_ms);
  s_manual = false;
  s_current = -2;
  prv_sync();
}

static void prv_back(ClickRecognizerRef r, void *ctx) {
  if (s_manual) {
    s_manual = false;
    s_focus = s_current < 0 ? 0 : s_current;
    layer_mark_dirty(s_layer);
  } else {
    window_stack_pop(true);
  }
}

static void prv_click_config(void *ctx) {
  window_single_repeating_click_subscribe(BUTTON_ID_UP, 150, prv_up);
  window_single_repeating_click_subscribe(BUTTON_ID_DOWN, 150, prv_down);
  window_single_click_subscribe(BUTTON_ID_SELECT, prv_select);
  window_single_click_subscribe(BUTTON_ID_BACK, prv_back);
}

#if defined(PBL_TOUCH)
static void prv_swipe(const Recognizer *recognizer, RecognizerEvent event) {
  if (event != RecognizerEvent_Completed || !touch_mode()) return;
  switch (swipe_recognizer_get_direction(recognizer)) {
    case SwipeDirection_Right: window_stack_pop(true); break;
    case SwipeDirection_Up: prv_move(1); break;
    case SwipeDirection_Down: prv_move(-1); break;
    default: break;
  }
}
#endif

static void prv_load(Window *window) {
  Layer *root = window_get_root_layer(window);
  window_set_background_color(window, GColorBlack);
  s_layer = layer_create(layer_get_bounds(root));
  layer_set_update_proc(s_layer, prv_draw);
  layer_add_child(root, s_layer);
  window_set_click_config_provider(window, prv_click_config);
#if defined(PBL_TOUCH)
  window_set_touch_bridge_disabled(window, true);
  window_attach_recognizer(window, swipe_recognizer_create(prv_swipe, NULL,
      SwipeDirection_Right | SwipeDirection_Up | SwipeDirection_Down));
#endif
  lyrics_track_changed();
}

static void prv_appear(Window *window) {
  touch_set_hold_handler(NULL, NULL);
}

static void prv_unload(Window *window) {
  if (s_follow_timer) app_timer_cancel(s_follow_timer);
  s_follow_timer = NULL;
  if (s_retry_timer) app_timer_cancel(s_retry_timer);
  s_retry_timer = NULL;
  layer_destroy(s_layer);
  s_layer = NULL;
  free(s_lines);
  s_lines = NULL;
  s_count = s_total = 0;
  s_loaded = false;
  s_waiting = false;
  window_destroy(s_window);
  s_window = NULL;
}

void lyrics_push(void) {
  idle_activity();
  if (s_window) return;
  s_lines = malloc(MAX_LINES * sizeof(Line));
  if (!s_lines) {
    show_notice("Out of memory");
    return;
  }
  s_window = window_create();
  window_set_window_handlers(s_window, (WindowHandlers){
    .load = prv_load, .appear = prv_appear, .unload = prv_unload,
  });
  window_stack_push(s_window, true);
}
