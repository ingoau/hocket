// Now Playing: a port of PebbleOS's system music app (fw/apps/system/music.c, Apache-2.0) to the
// public SDK, plus Hocket's touch gestures and the buttons-only back menu. Behaviour follows the
// reference section of docs/pebble.md; differences are noted where they happen. The biggest: the
// track's star rating sits where the reference's cassette was.
#include "hocket.h"

// The reference animations ran at 28fps.
#define FRAME_MS (1000 / 28)
#define VOLUME_REPEAT_INTERVAL_MS 400
#define ACTION_BAR_TIMEOUT_MS 2000
#define VOLUME_ICON_TIMEOUT_MS 2000
#define PROGRESS_FLICK_MS 5000
#define ART_REQUEST_DELAY_MS 500
#define ART_RETRY_DELAY_MS 3000
#define ART_MAX_RETRIES 2
#define BOUNCEBACK_OFFSET 6
#define TITLE_BOUNDS_OFFSET 5
#define ARTIST_BOUNDS_OFFSET 3
#define TIME_BOUNDS_OFFSET 2

#if defined(PBL_PLATFORM_EMERY) || defined(PBL_PLATFORM_GABBRO)
#define ART_SUPPORTED 1
#else
#define ART_SUPPORTED 0
#endif

typedef struct {
  const char *time_font;
  const char *artist_font;
  const char *title_font;
  const char *no_music_font;
  int16_t margin;
  int16_t artist_y, artist_h;
  int16_t title_y, title_h;
  int16_t time_y, time_h;
  int16_t star_radius;
  int16_t track_y, track_h, track_radius;
  GPoint no_music_pos;
  int16_t no_music_text_y, no_music_text_h;
} SizeConfig;

#define VOFF PBL_IF_RECT_ELSE(0, 5)

// The reference's Medium and Large configs; Large on displays at least 200px tall, as there.
static const SizeConfig MEDIUM = {
  .time_font = FONT_KEY_GOTHIC_14,
  .artist_font = FONT_KEY_GOTHIC_18_BOLD,
  .title_font = FONT_KEY_GOTHIC_24_BOLD,
  .no_music_font = FONT_KEY_GOTHIC_18_BOLD,
  .margin = PBL_IF_RECT_ELSE(12, 25),
  .artist_y = 31 + VOFF, .artist_h = 21,
  .title_y = 53 + VOFF, .title_h = 44,
  .time_y = 106 + VOFF, .time_h = 14,
  .star_radius = 7,
  .track_y = 120 + VOFF, .track_h = 4, .track_radius = 1,
  .no_music_pos = {PBL_IF_RECT_ELSE(29, 53), PBL_IF_RECT_ELSE(25, 26)},
  .no_music_text_y = PBL_IF_RECT_ELSE(107, 104), .no_music_text_h = 58,
};

static const SizeConfig LARGE = {
  .time_font = FONT_KEY_GOTHIC_18_BOLD,
  .artist_font = FONT_KEY_GOTHIC_24_BOLD,
  .title_font = FONT_KEY_GOTHIC_28,
  .no_music_font = FONT_KEY_GOTHIC_28,
  .margin = PBL_IF_RECT_ELSE(10, 30),
  .artist_y = PBL_IF_RECT_ELSE(30, 48), .artist_h = 21 + 8,
  .title_y = PBL_IF_RECT_ELSE(60, 72), .title_h = 80,
  .time_y = PBL_IF_RECT_ELSE(146, 158), .time_h = 20,
  .star_radius = 10,
  .track_y = PBL_IF_RECT_ELSE(168, 182), .track_h = 10, .track_radius = 4,
  .no_music_pos = {PBL_IF_RECT_ELSE(57, 72), PBL_IF_RECT_ELSE(46, 58)},
  .no_music_text_y = PBL_IF_RECT_ELSE(131, 143), .no_music_text_h = 58,
};

typedef enum { BarSkip, BarVolume, BarLongPress } BarState;

static Window *s_window;
static const SizeConfig *s_cfg;
static GSize s_size;

static StatusBarLayer *s_status;
static BitmapLayer *s_art_layer;
static Layer *s_art_text_layer;   // outlined artist over the cover
static Layer *s_art_title_layer;  // the title over the cover's foot, scrolling (clips to its frame)
static TextLayer *s_artist, *s_title, *s_position, *s_length;
static Layer *s_track;
static Layer *s_rating;  // the stars, or the volume feedback for a moment
static ActionBarLayer *s_bar;
static BitmapLayer *s_message_image;
static TextLayer *s_message_text;

static GBitmap *s_icon_skip_forward, *s_icon_skip_backward, *s_icon_ellipsis, *s_icon_play,
    *s_icon_pause, *s_icon_play_pause, *s_icon_volume_up, *s_icon_volume_down;
static GBitmap *s_image_volume_up, *s_image_volume_down, *s_image_no_music;
static GBitmap *s_art;

static char s_artist_buf[TEXT_LEN], s_title_buf[TEXT_LEN];
static char s_position_buf[12], s_length_buf[12];
static char s_message_buf[2 * TEXT_LEN];

static BarState s_bar_state;
static AppTimer *s_bar_revert_timer, *s_volume_repeat_timer, *s_volume_icon_timer;
static AppTimer *s_progress_flick_timer, *s_art_request_timer, *s_art_retry_timer;
static AppTimer *s_marquee_timer;
static bool s_volume_is_up;
static bool s_progress_flicked;
static bool s_showing_message;
static GBitmap *s_volume_image;  // shown instead of the stars while set
static Animation *s_transition;
static uint16_t s_art_requested_gen;
#if ART_SUPPORTED
static uint8_t s_art_retries;
#endif
static int16_t s_marquee_offset;
static uint8_t s_marquee_cycles;

static void prv_click_config_skip(void *context);
static void prv_click_config_volume(void *context);
static void prv_update_ui_state(bool animated);
static void prv_update_progress(void);
static void prv_update_layout(void);
static void prv_set_tick(void);

// -- Geometry --------------------------------------------------------------------------------------

static int16_t prv_content_width(void) {
  return s_size.w - ACTION_BAR_WIDTH - s_cfg->margin * 2;
}

static int16_t prv_text_width(void) {
  return prv_content_width() + PBL_IF_RECT_ELSE(s_cfg->margin / 2, 0);
}

static GRect prv_artist_rect(void) {
  return GRect(s_cfg->margin, s_cfg->artist_y, prv_text_width(), s_cfg->artist_h);
}

static GRect prv_title_rect(void) {
  return GRect(s_cfg->margin, s_cfg->title_y, prv_text_width(), s_cfg->title_h);
}

static GRect prv_time_rect(void) {
  return GRect(s_cfg->margin, s_cfg->time_y, prv_content_width(), s_cfg->time_h);
}

// The row under the progress bar, about midway between the times and the bottom of the screen; the
// stars start at the text margin (on round, inside the circle from the content margin).
static GRect prv_rating_rect(void) {
  const int16_t h = s_cfg->star_radius * 2 + 6;
  const int16_t below = s_cfg->track_y + s_cfg->track_h;
  const int16_t y = below + (s_size.h - below - h) / 2;
  int16_t x = s_cfg->margin;
#if PBL_ROUND
  // The circle is narrower down here: start where its edge is at the row's bottom.
  const int32_t r = s_size.w / 2, dy = y + h - s_size.h / 2;
  int32_t half = 0;
  while ((half + 1) * (half + 1) <= r * r - dy * dy) half++;
  if (r - half + 2 > x) x = r - half + 2;
#endif
  return GRect(x, y, s_size.w - ACTION_BAR_WIDTH - x - s_cfg->margin, h);
}

static GRect prv_track_rect(void) {
  return GRect(s_cfg->margin, s_cfg->track_y, prv_content_width(), s_cfg->track_h);
}

static bool prv_progress_visible(void) {
  return (g_settings.show_progress_bar || s_progress_flicked) && (g_now.flags & NowReportsPosition);
}

static bool prv_has_art(void) {
  return ART_SUPPORTED && s_art && g_settings.show_album_art;
}

// -- Text and progress -----------------------------------------------------------------------------

static void prv_format_time(char *buf, size_t n, uint32_t ms) {
  const uint32_t s = ms / 1000;
  const uint32_t h = s / 3600, m = (s % 3600) / 60, sec = s % 60;
  if (h) snprintf(buf, n, "%lu:%02lu:%02lu", (unsigned long)h, (unsigned long)m, (unsigned long)sec);
  else snprintf(buf, n, "%lu:%02lu", (unsigned long)m, (unsigned long)sec);
}

static void prv_track_update(Layer *layer, GContext *ctx) {
  const GRect b = layer_get_bounds(layer);
  graphics_context_set_fill_color(ctx, PBL_IF_COLOR_ELSE(GColorBlack, GColorWhite));
  graphics_fill_rect(ctx, b, s_cfg->track_radius, GCornersAll);
  uint32_t pct = 0;
  if (g_now.duration_ms) pct = (uint64_t)now_position_ms() * 100 / g_now.duration_ms;
  if (pct > 100) pct = 100;
  const int16_t w = b.size.w * pct / 100;
  if (w > 0) {
    graphics_context_set_fill_color(ctx, PBL_IF_COLOR_ELSE(HOCKET_ACCENT, GColorBlack));
    graphics_fill_rect(ctx, GRect(0, 0, w, b.size.h), s_cfg->track_radius, GCornersAll);
  }
#ifndef PBL_COLOR
  graphics_context_set_stroke_color(ctx, GColorBlack);
  graphics_draw_round_rect(ctx, b, s_cfg->track_radius);
#endif
}

static void prv_update_progress(void) {
  if (!s_window || !prv_progress_visible()) return;
  prv_format_time(s_position_buf, sizeof(s_position_buf), now_position_ms());
  prv_format_time(s_length_buf, sizeof(s_length_buf), g_now.duration_ms);
  layer_mark_dirty(text_layer_get_layer(s_position));
  layer_mark_dirty(text_layer_get_layer(s_length));
  layer_mark_dirty(s_track);
}

static void prv_tick(struct tm *t, TimeUnits units) {
  if (g_now.flags & NowPlaying) prv_update_progress();
}

static void prv_set_tick(void) {
  if (prv_progress_visible() && (g_now.flags & NowPlaying)) {
    tick_timer_service_subscribe(SECOND_UNIT, prv_tick);
  } else {
    tick_timer_service_unsubscribe();
  }
}

// Outlined text (8 offset black copies, then white), so it reads over the cover.
static void prv_draw_outlined(GContext *ctx, const char *text, GFont font, GRect box,
                              GTextOverflowMode overflow, GTextAlignment align) {
  graphics_context_set_text_color(ctx, GColorBlack);
  for (int dx = -1; dx <= 1; dx++) {
    for (int dy = -1; dy <= 1; dy++) {
      if (!dx && !dy) continue;
      GRect r = box;
      r.origin.x += dx;
      r.origin.y += dy;
      graphics_draw_text(ctx, text, font, r, overflow, align, NULL);
    }
  }
  graphics_context_set_text_color(ctx, GColorWhite);
  graphics_draw_text(ctx, text, font, box, overflow, align, NULL);
}

// Over art (rect): the artist outlined and centred near the bottom of the cover, and the title on
// one line under it, scrolling like a sign for a couple of cycles after a track change.
static GRect prv_art_title_rect(void) {
#if PBL_RECT
  // The cover's last line, the content's full width (the stars have the row below the times).
  const GRect art = layer_get_frame(bitmap_layer_get_layer(s_art_layer));
  return GRect(4, art.size.h - 30, art.size.w - 8, 28);
#else
  return GRect(40, s_size.h / 2 - 20, s_size.w - 80, 60);
#endif
}

static void prv_art_text_update(Layer *layer, GContext *ctx) {
  const GRect art = layer_get_frame(bitmap_layer_get_layer(s_art_layer));
  (void)art;
  GFont artist_font = fonts_get_system_font(s_cfg->artist_font);
#if PBL_RECT
  if (s_artist_buf[0]) {
    prv_draw_outlined(ctx, s_artist_buf, artist_font, GRect(0, art.size.h - 58, art.size.w, 28),
                      GTextOverflowModeTrailingEllipsis, GTextAlignmentCenter);
  }
#else
  if (s_artist_buf[0]) {
    prv_draw_outlined(ctx, s_artist_buf, artist_font, GRect(40, s_size.h / 2 - 76, s_size.w - 80, 30),
                      GTextOverflowModeTrailingEllipsis, GTextAlignmentCenter);
  }
  prv_draw_outlined(ctx, s_title_buf, fonts_get_system_font(FONT_KEY_GOTHIC_28_BOLD), prv_art_title_rect(),
                    GTextOverflowModeTrailingEllipsis, GTextAlignmentCenter);
#endif
}

static int16_t prv_title_overflow(void);

#if PBL_RECT
static void prv_art_title_update(Layer *layer, GContext *ctx) {
  const GRect b = layer_get_bounds(layer);
  // Centred while it fits; scrolling from the left edge when it doesn't.
  const bool fits = prv_title_overflow() <= 0;
  prv_draw_outlined(ctx, s_title_buf, fonts_get_system_font(FONT_KEY_GOTHIC_24_BOLD),
                    fits ? GRect(0, 0, b.size.w, b.size.h) : GRect(-s_marquee_offset, 0, 2000, b.size.h),
                    GTextOverflowModeFill, fits ? GTextAlignmentCenter : GTextAlignmentLeft);
}
#endif

static int16_t prv_title_overflow(void) {
  GRect t = prv_art_title_rect();
  GSize used = graphics_text_layout_get_content_size(
      s_title_buf, fonts_get_system_font(FONT_KEY_GOTHIC_24_BOLD), GRect(0, 0, 2000, t.size.h),
      GTextOverflowModeFill, GTextAlignmentLeft);
  return used.w - t.size.w;
}

static void prv_marquee_step(void *context) {
  s_marquee_timer = NULL;
  const int16_t overflow = prv_title_overflow();
  if (overflow <= 0 || !prv_has_art()) {
    s_marquee_offset = 0;
    return;
  }
  uint32_t delay = 60;
  if (s_marquee_offset >= overflow + 20) {
    s_marquee_offset = 0;
    if (++s_marquee_cycles >= 2) {
      layer_mark_dirty(s_art_title_layer);
      return;  // rest at the start
    }
    delay = 1500;
  } else {
    s_marquee_offset += 2;
    if (s_marquee_offset == 2) delay = 1000;
  }
  layer_mark_dirty(s_art_title_layer);
  s_marquee_timer = app_timer_register(delay, prv_marquee_step, NULL);
}

static void prv_restart_marquee(void) {
  if (s_marquee_timer) app_timer_cancel(s_marquee_timer);
  s_marquee_timer = NULL;
  s_marquee_offset = 0;
  s_marquee_cycles = 0;
  if (prv_has_art() && PBL_IF_RECT_ELSE(true, false)) {
    s_marquee_timer = app_timer_register(1000, prv_marquee_step, NULL);
  }
}

// -- Rating (where the reference had its cassette) -------------------------------------------------

// A five-pointed star of outer radius r centred on c, filled or outlined.
static void prv_draw_star(GContext *ctx, GPoint c, int16_t r, bool filled) {
  GPoint points[10];
  for (int i = 0; i < 10; i++) {
    const int32_t angle = TRIG_MAX_ANGLE * i / 10;
    const int32_t radius = i % 2 ? r * 2 / 5 : r;
    points[i] = GPoint(c.x + sin_lookup(angle) * radius / TRIG_MAX_RATIO,
                       c.y - cos_lookup(angle) * radius / TRIG_MAX_RATIO);
  }
  GPath path = {.num_points = 10, .points = points};
  if (filled) {
    graphics_context_set_fill_color(ctx, PBL_IF_COLOR_ELSE(HOCKET_ACCENT, GColorBlack));
    gpath_draw_filled(ctx, &path);
  }
  graphics_context_set_stroke_color(ctx, GColorBlack);
  gpath_draw_outline(ctx, &path);
}

static void prv_rating_update(Layer *layer, GContext *ctx) {
  const GRect b = layer_get_bounds(layer);
  if (s_volume_image) {
    const GSize img = gbitmap_get_bounds(s_volume_image).size;
    graphics_context_set_compositing_mode(ctx, GCompOpSet);
    graphics_draw_bitmap_in_rect(ctx, s_volume_image, GRect(0, (b.size.h - img.h) / 2, img.w, img.h));
    return;
  }
  const int16_t r = s_cfg->star_radius;
  const int16_t step = r * 2 + 4;
  const int16_t x0 = r + 1;
  for (int i = 0; i < 5; i++) {
#ifndef PBL_COLOR
    // Black and white: only the filled stars, so the count reads at a glance.
    if (i >= g_now.rating) break;
#endif
    prv_draw_star(ctx, GPoint(x0 + i * step, b.size.h / 2), r, i < g_now.rating);
  }
}

static void prv_volume_icon_timeout(void *context) {
  s_volume_icon_timer = NULL;
  s_volume_image = NULL;
  layer_mark_dirty(s_rating);
}

// The reference's volume feedback: the big volume icon for a couple of seconds, then the stars.
static void prv_show_volume_image(bool up) {
  if (s_volume_icon_timer) app_timer_reschedule(s_volume_icon_timer, VOLUME_ICON_TIMEOUT_MS);
  else s_volume_icon_timer = app_timer_register(VOLUME_ICON_TIMEOUT_MS, prv_volume_icon_timeout, NULL);
  s_volume_image = up ? s_image_volume_up : s_image_volume_down;
  layer_mark_dirty(s_rating);
}

static Animation *prv_layer_up(Layer *layer, int16_t offset) {
  GPoint from = GPoint(0, -offset);
  GPoint to = GPoint(0, -layer_get_bounds(layer).size.h - offset);
  PropertyAnimation *a = property_animation_create_bounds_origin(layer, &from, &to);
  animation_set_duration(property_animation_get_animation(a), 4 * FRAME_MS);
  animation_set_curve(property_animation_get_animation(a), AnimationCurveEaseIn);
  return property_animation_get_animation(a);
}

static Animation *prv_layer_bounce(Layer *layer, GRect target) {
  GRect from = target;
  from.origin.y -= BOUNCEBACK_OFFSET;
  PropertyAnimation *a = property_animation_create_layer_frame(layer, &from, &target);
  animation_set_duration(property_animation_get_animation(a), 4 * FRAME_MS);
  animation_set_curve(property_animation_get_animation(a), AnimationCurveEaseOut);
  return property_animation_get_animation(a);
}

static void prv_reset_text_bounds(void) {
  struct { TextLayer *t; int16_t off; } layers[] = {
    {s_artist, ARTIST_BOUNDS_OFFSET}, {s_title, TITLE_BOUNDS_OFFSET},
    {s_position, TIME_BOUNDS_OFFSET}, {s_length, TIME_BOUNDS_OFFSET},
  };
  for (unsigned i = 0; i < ARRAY_LENGTH(layers); i++) {
    Layer *l = text_layer_get_layer(layers[i].t);
    GRect b = layer_get_bounds(l);
    b.origin.y = -layers[i].off;
    layer_set_bounds(l, b);
  }
}

static void prv_flip_text(Animation *animation, bool finished, void *context) {
  strncpy(s_artist_buf, g_now.artist, sizeof(s_artist_buf));
  strncpy(s_title_buf, g_now.title, sizeof(s_title_buf));
  prv_reset_text_bounds();
  prv_update_progress();
}

static void prv_transition_done(Animation *animation, bool finished, void *context) {
  s_transition = NULL;
}

// The text slides up out of view, swaps, and bounces back in (rect, no art).
static void prv_track_change_animation(void) {
  if (s_transition) return;
  Animation *up = animation_spawn_create(
      prv_layer_up(text_layer_get_layer(s_artist), ARTIST_BOUNDS_OFFSET),
      prv_layer_up(text_layer_get_layer(s_title), TITLE_BOUNDS_OFFSET),
      prv_layer_up(text_layer_get_layer(s_position), TIME_BOUNDS_OFFSET),
      prv_layer_up(text_layer_get_layer(s_length), TIME_BOUNDS_OFFSET), NULL);
  animation_set_handlers(up, (AnimationHandlers){.stopped = prv_flip_text}, NULL);
  const GRect time = prv_time_rect();
  Animation *bounce = animation_spawn_create(
      prv_layer_bounce(text_layer_get_layer(s_artist), prv_artist_rect()),
      prv_layer_bounce(text_layer_get_layer(s_title), prv_title_rect()),
      prv_layer_bounce(text_layer_get_layer(s_position), time),
      prv_layer_bounce(text_layer_get_layer(s_length), time), NULL);
  s_transition = animation_sequence_create(up, bounce, NULL);
  animation_set_handlers(s_transition, (AnimationHandlers){.stopped = prv_transition_done}, NULL);
  animation_schedule(s_transition);
}

// -- Action bar --------------------------------------------------------------------------------------

static void prv_update_ui_state(bool animated) {
  if (!s_window) return;
  if (s_showing_message) {
    action_bar_layer_clear_icon(s_bar, BUTTON_ID_UP);
    action_bar_layer_clear_icon(s_bar, BUTTON_ID_DOWN);
    if (!g_version_mismatch && (g_now.flags & NowCanResume)) {
      action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_SELECT, s_icon_play, animated);
    } else {
      action_bar_layer_clear_icon(s_bar, BUTTON_ID_SELECT);
    }
    return;
  }
  const bool playing = g_now.flags & NowPlaying;
  if (s_bar_state == BarSkip) {
    action_bar_layer_set_click_config_provider(s_bar, prv_click_config_skip);
    action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_UP, s_icon_skip_backward, animated);
    action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_DOWN, s_icon_skip_forward, animated);
    GBitmap *select = !playing ? s_icon_play
                      : g_settings.show_volume_controls ? s_icon_ellipsis : s_icon_pause;
    action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_SELECT, select, animated);
  } else {
    if (s_bar_state == BarVolume) action_bar_layer_set_click_config_provider(s_bar, prv_click_config_volume);
    action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_UP, s_icon_volume_up, animated);
    action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_DOWN, s_icon_volume_down, animated);
    action_bar_layer_set_icon_animated(s_bar, BUTTON_ID_SELECT, playing ? s_icon_pause : s_icon_play, animated);
  }
}

static void prv_set_bar_state(BarState state) {
  s_bar_state = state;
  prv_update_ui_state(true);
}

static void prv_bar_revert(void *context) {
  s_bar_revert_timer = NULL;
  prv_set_bar_state(BarSkip);
}

static void prv_reset_bar_revert(void) {
  if (s_bar_revert_timer) app_timer_reschedule(s_bar_revert_timer, ACTION_BAR_TIMEOUT_MS);
}

static void prv_toggle_playing(void) {
  if (s_showing_message) {
    if (g_now.flags & NowCanResume) send_cmd(CmdResume, 0, NULL, NULL);
    return;
  }
  send_cmd(CmdPlayPause, 0, NULL, NULL);
  g_now.flags ^= NowPlaying;  // optimistic; the phone's next state confirms
  time_ms(&g_now.position_at, &g_now.position_at_ms);
  prv_update_ui_state(true);
  prv_set_tick();
}

static void prv_change_volume(bool up) {
  if (!(g_now.flags & NowHasVolume)) return;
  prv_show_volume_image(up);
  send_cmd(up ? CmdVolumeUp : CmdVolumeDown, 0, NULL, NULL);
}

static void prv_skip(ButtonId button) {
  if (s_showing_message) return;
  send_cmd(button == BUTTON_ID_UP ? CmdPrevious : CmdNext, 0, NULL, NULL);
}

static void prv_select(void) {
  if (s_showing_message) {
    prv_toggle_playing();
    return;
  }
  if (s_bar_state != BarSkip) {
    prv_reset_bar_revert();
    prv_toggle_playing();
  } else if ((g_now.flags & NowPlaying) && g_settings.show_volume_controls) {
    // The "…" opens volume mode for a couple of seconds.
    s_bar_revert_timer = app_timer_register(ACTION_BAR_TIMEOUT_MS, prv_bar_revert, NULL);
    prv_set_bar_state(BarVolume);
  } else {
    prv_toggle_playing();
  }
}

static void prv_skip_click(ClickRecognizerRef recognizer, void *context) {
  prv_skip(click_recognizer_get_button_id(recognizer));
}

static void prv_select_click(ClickRecognizerRef recognizer, void *context) {
  prv_select();
}

static void prv_volume_click(ClickRecognizerRef recognizer, void *context) {
  prv_reset_bar_revert();
  prv_change_volume(click_recognizer_get_button_id(recognizer) == BUTTON_ID_UP);
  if (click_number_of_clicks_counted(recognizer) >= 2) vibes_enqueue_custom_pattern((VibePattern){.durations = (uint32_t[]){20}, .num_segments = 1});
}

static void prv_volume_repeat(void *context) {
  if (!s_volume_repeat_timer) return;
  s_volume_repeat_timer = app_timer_register(VOLUME_REPEAT_INTERVAL_MS, prv_volume_repeat, NULL);
  prv_change_volume(s_volume_is_up);
  vibes_enqueue_custom_pattern((VibePattern){.durations = (uint32_t[]){20}, .num_segments = 1});
}

static void prv_volume_long_start(ClickRecognizerRef recognizer, void *context) {
  if (s_showing_message) return;
  s_volume_is_up = click_recognizer_get_button_id(recognizer) == BUTTON_ID_UP;
  prv_change_volume(s_volume_is_up);
  prv_set_bar_state(BarLongPress);
  s_volume_repeat_timer = app_timer_register(VOLUME_REPEAT_INTERVAL_MS, prv_volume_repeat, NULL);
  vibes_enqueue_custom_pattern((VibePattern){.durations = (uint32_t[]){20}, .num_segments = 1});
}

static void prv_volume_long_end(ClickRecognizerRef recognizer, void *context) {
  if (s_bar_state != BarLongPress) return;
  prv_set_bar_state(BarSkip);
  if (s_volume_repeat_timer) app_timer_cancel(s_volume_repeat_timer);
  s_volume_repeat_timer = NULL;
}

static void prv_play_pause_long_start(ClickRecognizerRef recognizer, void *context) {
  if (s_showing_message) return;
  prv_toggle_playing();
  prv_set_bar_state(BarLongPress);
  vibes_enqueue_custom_pattern((VibePattern){.durations = (uint32_t[]){20}, .num_segments = 1});
}

static void prv_play_pause_long_end(ClickRecognizerRef recognizer, void *context) {
  if (s_bar_state == BarLongPress) prv_set_bar_state(BarSkip);
}

// Buttons-only mode: back opens the menu instead of leaving the app.
static void prv_back_click(ClickRecognizerRef recognizer, void *context) {
  list_push_menu(ListMenu);
}

static void prv_subscribe_back(void) {
  if (!touch_mode()) window_single_click_subscribe(BUTTON_ID_BACK, prv_back_click);
}

static void prv_click_config_skip(void *context) {
  window_single_click_subscribe(BUTTON_ID_UP, prv_skip_click);
  window_single_click_subscribe(BUTTON_ID_DOWN, prv_skip_click);
  window_single_click_subscribe(BUTTON_ID_SELECT, prv_select_click);
  if (g_settings.show_volume_controls) {
    window_long_click_subscribe(BUTTON_ID_UP, 0, prv_volume_long_start, prv_volume_long_end);
    window_long_click_subscribe(BUTTON_ID_DOWN, 0, prv_volume_long_start, prv_volume_long_end);
    window_long_click_subscribe(BUTTON_ID_SELECT, 0, prv_play_pause_long_start, prv_play_pause_long_end);
  }
  prv_subscribe_back();
}

static void prv_click_config_volume(void *context) {
  window_single_repeating_click_subscribe(BUTTON_ID_UP, VOLUME_REPEAT_INTERVAL_MS, prv_volume_click);
  window_single_repeating_click_subscribe(BUTTON_ID_DOWN, VOLUME_REPEAT_INTERVAL_MS, prv_volume_click);
  window_single_click_subscribe(BUTTON_ID_SELECT, prv_select_click);
  prv_subscribe_back();
}

// -- Touch -----------------------------------------------------------------------------------------

#if defined(PBL_TOUCH)
static void prv_swipe(const Recognizer *recognizer, RecognizerEvent event) {
  if (event != RecognizerEvent_Completed || !touch_mode()) return;
  switch (swipe_recognizer_get_direction(recognizer)) {
    case SwipeDirection_Up: open_gesture(g_settings.gestures[GestureUp]); break;
    case SwipeDirection_Down: open_gesture(g_settings.gestures[GestureDown]); break;
    case SwipeDirection_Left: open_gesture(g_settings.gestures[GestureLeft]); break;
    case SwipeDirection_Right: open_gesture(g_settings.gestures[GestureRight]); break;
    default: break;
  }
}

// With touch navigation on, taps on the action-bar icons press that button; taps elsewhere (the
// cover, the text) do nothing, as in the reference.
static void prv_tap(const Recognizer *recognizer, RecognizerEvent event) {
  if (event != RecognizerEvent_Completed || !touch_mode() || !g_settings.touch_navigation) return;
  const GPoint p = tap_recognizer_get_tap_point(recognizer);
  if (p.x < s_size.w - ACTION_BAR_WIDTH) return;
  const int zone = p.y * 3 / s_size.h;
  if (zone == 1) {
    prv_select();
  } else if (s_bar_state == BarSkip) {
    prv_skip(zone == 0 ? BUTTON_ID_UP : BUTTON_ID_DOWN);
  } else {
    prv_reset_bar_revert();
    prv_change_volume(zone == 0);
  }
}
#endif

static void prv_hold(void *context) {
  open_gesture(g_settings.gestures[GestureHold]);
}

// -- Layout ----------------------------------------------------------------------------------------

static void prv_flick_timeout(void *context) {
  s_progress_flick_timer = NULL;
  s_progress_flicked = false;
  prv_update_layout();
}

// With the progress bar off, a flick of the wrist shows it for a few seconds.
static void prv_accel_tap(AccelAxisType axis, int32_t direction) {
  s_progress_flicked = true;
  if (s_progress_flick_timer) app_timer_cancel(s_progress_flick_timer);
  s_progress_flick_timer = app_timer_register(PROGRESS_FLICK_MS, prv_flick_timeout, NULL);
  prv_update_layout();
}

static void prv_update_layout(void) {
  if (!s_window) return;
  // The message (nothing playing, waiting, update) replaces the track info.
  const bool message = g_version_mismatch || !(g_now.flags & NowHasTrack);
  s_showing_message = message;
  if (message) {
    if (g_version_mismatch) {
      strcpy(s_message_buf, "UPDATE FROM HOCKET\nON YOUR PHONE");
    } else if (!g_phone_seen) {
      strcpy(s_message_buf, "OPEN HOCKET\nON YOUR PHONE");
    } else if (g_now.flags & NowCanResume) {
      snprintf(s_message_buf, sizeof(s_message_buf), "RESUME\n%s", g_now.resume_label);
    } else {
      strcpy(s_message_buf, "NOTHING PLAYING");
    }
    text_layer_set_text(s_message_text, s_message_buf);
  }
  layer_set_hidden(bitmap_layer_get_layer(s_message_image), !message);
  layer_set_hidden(text_layer_get_layer(s_message_text), !message);
  const bool art = !message && prv_has_art();
  bitmap_layer_set_bitmap(s_art_layer, art ? s_art : NULL);
  layer_set_hidden(bitmap_layer_get_layer(s_art_layer), !art);
  layer_set_hidden(s_art_text_layer, !art);
  layer_set_hidden(s_art_title_layer, !art || !PBL_IF_RECT_ELSE(true, false));
  layer_set_hidden(text_layer_get_layer(s_artist), message || art);
  layer_set_hidden(text_layer_get_layer(s_title), message || art);
  layer_set_hidden(s_rating, message);
  layer_mark_dirty(s_rating);
  const bool progress = !message && prv_progress_visible();
  layer_set_hidden(s_track, !progress);
  layer_set_hidden(text_layer_get_layer(s_position), !progress);
  layer_set_hidden(text_layer_get_layer(s_length), !progress);
  status_bar_layer_set_colors(s_status, GColorClear, art ? GColorWhite : GColorBlack);
  prv_update_progress();
  prv_update_ui_state(false);
  prv_set_tick();
}

// -- Album art requests -----------------------------------------------------------------------------

#if ART_SUPPORTED
static void prv_request_art(void *context) {
  s_art_request_timer = NULL;
  if (!g_settings.show_album_art || !g_now.art_gen) return;
  s_art_requested_gen = g_now.art_gen;
  GRect r = layer_get_frame(bitmap_layer_get_layer(s_art_layer));
  send_art_request(g_now.art_gen, r.size.w, r.size.h);
  if (s_art_retry_timer) app_timer_cancel(s_art_retry_timer);
  s_art_retry_timer = app_timer_register(ART_RETRY_DELAY_MS, prv_request_art, (void *)1);
  if (context) {
    if (++s_art_retries > ART_MAX_RETRIES) {
      app_timer_cancel(s_art_retry_timer);
      s_art_retry_timer = NULL;
    }
  }
}
#endif

static void prv_art_gen_changed(void) {
#if ART_SUPPORTED
  if (s_art_request_timer) app_timer_cancel(s_art_request_timer);
  if (s_art_retry_timer) app_timer_cancel(s_art_retry_timer);
  s_art_request_timer = s_art_retry_timer = NULL;
  s_art_retries = 0;
  if (!g_now.art_gen) {
    now_playing_art_ready(NULL);
    return;
  }
  // Coalesce the burst of state updates a track change brings. The old cover stays meanwhile.
  s_art_request_timer = app_timer_register(ART_REQUEST_DELAY_MS, prv_request_art, NULL);
#endif
}

void now_playing_art_ready(GBitmap *art) {
  if (s_art_retry_timer) {
    app_timer_cancel(s_art_retry_timer);
    s_art_retry_timer = NULL;
  }
  bitmap_layer_set_bitmap(s_art_layer, NULL);
  if (s_art) gbitmap_destroy(s_art);
  s_art = art;
  prv_update_layout();
  prv_restart_marquee();
}

// -- Public ------------------------------------------------------------------------------------------

void now_playing_update(bool track_changed) {
  if (!s_window) return;
  const bool animate = track_changed && !prv_has_art() && s_title_buf[0] && PBL_IF_RECT_ELSE(true, false)
                       && window_stack_get_top_window() == s_window;
  if (animate) {
    prv_track_change_animation();
  } else if (!s_transition) {
    strncpy(s_artist_buf, g_now.artist, sizeof(s_artist_buf));
    strncpy(s_title_buf, g_now.title, sizeof(s_title_buf));
    layer_mark_dirty(text_layer_get_layer(s_title));
    layer_mark_dirty(text_layer_get_layer(s_artist));
  }
  if (track_changed) prv_restart_marquee();
  if (g_now.art_gen != s_art_requested_gen) prv_art_gen_changed();
  prv_update_layout();
  if (s_art_text_layer) layer_mark_dirty(s_art_text_layer);
  if (s_art_title_layer) layer_mark_dirty(s_art_title_layer);
}

void now_playing_settings_changed(void) {
  if (!s_window) return;
  if (!g_settings.show_progress_bar && (g_now.flags & NowReportsPosition)) {
    accel_tap_service_subscribe(prv_accel_tap);
  } else {
    accel_tap_service_unsubscribe();
  }
  if (g_settings.show_album_art && !s_art) {
    s_art_requested_gen = 0;
    prv_art_gen_changed();
  }
  s_bar_state = BarSkip;
  prv_update_layout();
}

Window *now_playing_window(void) {
  return s_window;
}

static TextLayer *prv_text(GRect rect, int16_t offset, GTextAlignment align, const char *font, char *buf) {
  TextLayer *t = text_layer_create(rect);
  text_layer_set_text(t, buf);
  text_layer_set_font(t, fonts_get_system_font(font));
  text_layer_set_text_color(t, GColorBlack);
  text_layer_set_background_color(t, GColorClear);
  text_layer_set_text_alignment(t, align);
  text_layer_set_overflow_mode(t, GTextOverflowModeFill);
  layer_set_bounds(text_layer_get_layer(t), GRect(0, -offset, rect.size.w, rect.size.h + offset));
  layer_add_child(window_get_root_layer(s_window), text_layer_get_layer(t));
  return t;
}

static void prv_load(Window *window) {
  Layer *root = window_get_root_layer(window);
  s_size = layer_get_bounds(root).size;
  s_cfg = s_size.h >= 200 ? &LARGE : &MEDIUM;
  window_set_background_color(window, PBL_IF_COLOR_ELSE(GColorLightGray, GColorWhite));

  const GRect time_rect = prv_time_rect();
  // The cover: a band from the top to just above the times on rect, the whole circle on round.
  const GRect art_rect = GRect(0, 0, PBL_IF_RECT_ELSE(s_size.w - ACTION_BAR_WIDTH, s_size.w),
                               PBL_IF_RECT_ELSE(time_rect.origin.y - 2, s_size.h));
  s_art_layer = bitmap_layer_create(art_rect);
  bitmap_layer_set_alignment(s_art_layer, GAlignCenter);
  layer_add_child(root, bitmap_layer_get_layer(s_art_layer));

  const GTextAlignment align = PBL_IF_RECT_ELSE(GTextAlignmentLeft, GTextAlignmentRight);
  s_artist = prv_text(prv_artist_rect(), ARTIST_BOUNDS_OFFSET, align, s_cfg->artist_font, s_artist_buf);
  s_position = prv_text(time_rect, TIME_BOUNDS_OFFSET, GTextAlignmentLeft, s_cfg->time_font, s_position_buf);
  s_length = prv_text(time_rect, TIME_BOUNDS_OFFSET, GTextAlignmentRight, s_cfg->time_font, s_length_buf);
  s_title = prv_text(prv_title_rect(), TITLE_BOUNDS_OFFSET, align, s_cfg->title_font, s_title_buf);
  text_layer_set_overflow_mode(s_title, GTextOverflowModeTrailingEllipsis);

  s_rating = layer_create(prv_rating_rect());
  layer_set_update_proc(s_rating, prv_rating_update);
  layer_add_child(root, s_rating);

  s_art_text_layer = layer_create(layer_get_bounds(root));
  layer_set_update_proc(s_art_text_layer, prv_art_text_update);
  layer_add_child(root, s_art_text_layer);
  s_art_title_layer = layer_create(prv_art_title_rect());
#if PBL_RECT
  layer_set_update_proc(s_art_title_layer, prv_art_title_update);
#endif
  layer_add_child(root, s_art_title_layer);

  s_track = layer_create(prv_track_rect());
  layer_set_update_proc(s_track, prv_track_update);
  layer_add_child(root, s_track);

  // Nothing playing / waiting / update: the reference's no-music picture and a line of text.
  const GSize img = gbitmap_get_bounds(s_image_no_music).size;
  s_message_image = bitmap_layer_create(GRect(s_cfg->no_music_pos.x, s_cfg->no_music_pos.y, img.w, img.h));
  bitmap_layer_set_bitmap(s_message_image, s_image_no_music);
  bitmap_layer_set_compositing_mode(s_message_image, GCompOpSet);
  layer_add_child(root, bitmap_layer_get_layer(s_message_image));
  s_message_text = text_layer_create(GRect(0, s_cfg->no_music_text_y, s_size.w - ACTION_BAR_WIDTH, s_cfg->no_music_text_h));
  text_layer_set_font(s_message_text, fonts_get_system_font(s_cfg->no_music_font));
  text_layer_set_background_color(s_message_text, GColorClear);
  text_layer_set_text_alignment(s_message_text, GTextAlignmentCenter);
  text_layer_set_overflow_mode(s_message_text, GTextOverflowModeTrailingEllipsis);
  layer_add_child(root, text_layer_get_layer(s_message_text));

  s_bar = action_bar_layer_create();
  action_bar_layer_add_to_window(s_bar, window);

  s_status = status_bar_layer_create();
  GRect sf = layer_get_frame(status_bar_layer_get_layer(s_status));
  sf.size.w = PBL_IF_RECT_ELSE(s_size.w - ACTION_BAR_WIDTH, s_size.w);
  layer_set_frame(status_bar_layer_get_layer(s_status), sf);
  status_bar_layer_set_colors(s_status, GColorClear, GColorBlack);
  layer_add_child(root, status_bar_layer_get_layer(s_status));

#if defined(PBL_TOUCH)
  // Our gestures replace the system's swipe-to-button mapping on this window.
  window_set_touch_bridge_disabled(window, true);
  window_attach_recognizer(window, swipe_recognizer_create(prv_swipe, NULL,
      SwipeDirection_Up | SwipeDirection_Down | SwipeDirection_Left | SwipeDirection_Right));
  window_attach_recognizer(window, tap_recognizer_create(prv_tap, NULL));
#endif

  strncpy(s_artist_buf, g_now.artist, sizeof(s_artist_buf));
  strncpy(s_title_buf, g_now.title, sizeof(s_title_buf));
  now_playing_settings_changed();
}

static void prv_appear(Window *window) {
  touch_set_hold_handler(prv_hold, NULL);
  s_bar_state = BarSkip;
  prv_update_layout();
}

static void prv_unload(Window *window) {
  tick_timer_service_unsubscribe();
  accel_tap_service_unsubscribe();
  status_bar_layer_destroy(s_status);
  action_bar_layer_destroy(s_bar);
  text_layer_destroy(s_message_text);
  bitmap_layer_destroy(s_message_image);
  layer_destroy(s_track);
  layer_destroy(s_art_title_layer);
  layer_destroy(s_art_text_layer);
  layer_destroy(s_rating);
  text_layer_destroy(s_title);
  text_layer_destroy(s_length);
  text_layer_destroy(s_position);
  text_layer_destroy(s_artist);
  bitmap_layer_destroy(s_art_layer);
  if (s_art) gbitmap_destroy(s_art);
  s_art = NULL;
  window_destroy(s_window);
  s_window = NULL;
}

void now_playing_push(void) {
  s_icon_skip_forward = gbitmap_create_with_resource(RESOURCE_ID_ICON_SKIP_FORWARD);
  s_icon_skip_backward = gbitmap_create_with_resource(RESOURCE_ID_ICON_SKIP_BACKWARD);
  s_icon_ellipsis = gbitmap_create_with_resource(RESOURCE_ID_ICON_ELLIPSIS);
  s_icon_play = gbitmap_create_with_resource(RESOURCE_ID_ICON_PLAY);
  s_icon_pause = gbitmap_create_with_resource(RESOURCE_ID_ICON_PAUSE);
  s_icon_play_pause = gbitmap_create_with_resource(RESOURCE_ID_ICON_PLAY_PAUSE);
  s_icon_volume_up = gbitmap_create_with_resource(RESOURCE_ID_ICON_VOLUME_UP);
  s_icon_volume_down = gbitmap_create_with_resource(RESOURCE_ID_ICON_VOLUME_DOWN);
  s_image_volume_up = gbitmap_create_with_resource(RESOURCE_ID_LARGE_VOLUME_UP);
  s_image_volume_down = gbitmap_create_with_resource(RESOURCE_ID_LARGE_VOLUME_DOWN);
  s_image_no_music = gbitmap_create_with_resource(RESOURCE_ID_NO_MUSIC);

  s_window = window_create();
  window_set_window_handlers(s_window, (WindowHandlers){
    .load = prv_load, .appear = prv_appear, .unload = prv_unload,
  });
  window_stack_push(s_window, true);
}
