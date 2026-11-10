// Click-through animations drawn over the desktop: the countdown while the cursor waits
// at a screen edge, and the highlight that shows where the cursor is.
//
// Every effect is one drawing function in a table, indexed by the number its enum has in
// config.rs (EdgeAnimStyle, LocateStyle). To add an effect: write the function, add it to
// the table, add the enum variant and its labels.
//
// All window work happens on the main thread; the fs_overlay_* entry points may be called
// from any thread and only queue it. They need a running AppKit event loop.

#import <Cocoa/Cocoa.h>
#import <QuartzCore/QuartzCore.h>
#include <math.h>
#include <stdbool.h>
#include <string.h>

enum { EDGE_LEFT, EDGE_RIGHT, EDGE_TOP, EDGE_BOTTOM };
enum { DIR_CLOCKWISE, DIR_COUNTERCLOCKWISE, DIR_BOTH };

/// How far into the screen the edge overlay reaches.
static const CGFloat kStripDepth = 360;
static const CGFloat kLocateSide = 420;
static const double kLocateSeconds = 1.3;
static const double kLocateFade = 0.35;

// ---- Drawing helpers -------------------------------------------------------------

/// The colour of the effect being drawn, as chosen in the settings.
static CGFloat gAccent[3] = { 1.0, 0.8, 0.0 };

static void stroke_accent(CGContextRef c, double alpha) {
    CGContextSetRGBStrokeColor(c, gAccent[0], gAccent[1], gAccent[2], alpha);
}

static void fill_accent(CGContextRef c, double alpha) {
    CGContextSetRGBFillColor(c, gAccent[0], gAccent[1], gAccent[2], alpha);
}

/// The chosen size of the effect being drawn. Shapes follow it through the scaled
/// drawing, but a shadow's blur does not (it is in device space), so the glow is
/// widened by hand.
static CGFloat gGlowScale = 1;

static void glow(CGContextRef c, double blur, double alpha) {
    CGColorRef color = CGColorCreateSRGB(gAccent[0], gAccent[1], gAccent[2], alpha);
    CGContextSetShadowWithColor(c, CGSizeZero, blur * gGlowScale, color);
    CGColorRelease(color);
}

static void no_glow(CGContextRef c) {
    CGContextSetShadowWithColor(c, CGSizeZero, 0, NULL);
}

static void circle_path(CGContextRef c, CGPoint o, double r) {
    CGContextAddArc(c, o.x, o.y, r, 0, 2 * M_PI, 0);
}

/// Repeatable noise in 0..1, so particles and sparks stay put from frame to frame.
static double noise(int n) {
    double s = sin(n * 127.1) * 43758.5453;
    return s - floor(s);
}

static double clamp01(double v) {
    return v < 0 ? 0 : (v > 1 ? 1 : v);
}

static double smooth(double from, double to, double v) {
    double x = clamp01((v - from) / (to - from));
    return x * x * (3 - 2 * x);
}

// ---- Edge countdown effects ------------------------------------------------------

typedef struct {
    CGContextRef c;
    CGSize size;
    /// The cursor, projected onto the screen edge.
    CGPoint anchor;
    /// Unit vector pointing into the screen.
    CGVector inward;
    /// Unit vector along the edge.
    CGVector along;
    /// Share of the wait that has passed (0..1); at 1 the effect has gone.
    double p;
    /// Seconds since the wait began.
    double t;
    int direction;
    bool shrink;
    /// The chosen size; the drawing is scaled by it around the anchor.
    double zoom;
} EdgeFrame;

typedef void (*EdgeRenderer)(const EdgeFrame *);

/// A point `a` along the edge and `d` into the screen, from the cursor.
static CGPoint from_anchor(const EdgeFrame *f, double a, double d) {
    return CGPointMake(f->anchor.x + f->along.dx * a + f->inward.dx * d,
                       f->anchor.y + f->along.dy * a + f->inward.dy * d);
}

static void edge_ring(const EdgeFrame *f) {
    CGContextRef c = f->c;
    const double radius = 24;
    double left = 1 - f->p;
    if (left <= 0.002) return;
    CGPoint o = from_anchor(f, 0, radius + 18);
    CGContextSetLineCap(c, kCGLineCapRound);
    CGContextSetLineWidth(c, 5);
    if (f->shrink) {
        glow(c, 12, 0.9);
        stroke_accent(c, 1);
        circle_path(c, o, radius * left);
        CGContextStrokePath(c);
        return;
    }
    stroke_accent(c, 0.18);
    circle_path(c, o, radius);
    CGContextStrokePath(c);
    glow(c, 12, 0.9);
    stroke_accent(c, 1);
    const double top = M_PI_2;
    switch (f->direction) {
        case DIR_COUNTERCLOCKWISE:
            CGContextAddArc(c, o.x, o.y, radius, top + 2 * M_PI * f->p, top + 2 * M_PI, 0);
            break;
        case DIR_BOTH:
            CGContextAddArc(c, o.x, o.y, radius, -M_PI_2 - M_PI * left, -M_PI_2 + M_PI * left, 0);
            break;
        default:
            CGContextAddArc(c, o.x, o.y, radius, top - 2 * M_PI * f->p, top - 2 * M_PI, 1);
    }
    CGContextStrokePath(c);
}

static void edge_semicircle(const EdgeFrame *f) {
    CGContextRef c = f->c;
    const double radius = 54;
    double left = 1 - f->p;
    if (left <= 0.002) return;
    CGPoint o = f->anchor;
    double mid = atan2(f->inward.dy, f->inward.dx);
    CGContextSetLineCap(c, kCGLineCapRound);
    if (f->shrink) {
        glow(c, 14, 0.8);
        fill_accent(c, 0.85);
        CGContextMoveToPoint(c, o.x, o.y);
        CGContextAddArc(c, o.x, o.y, radius * left, mid - M_PI_2, mid + M_PI_2, 0);
        CGContextClosePath(c);
        CGContextFillPath(c);
        return;
    }
    CGContextSetLineWidth(c, 6);
    stroke_accent(c, 0.18);
    CGContextAddArc(c, o.x, o.y, radius, mid - M_PI_2, mid + M_PI_2, 0);
    CGContextStrokePath(c);

    double from, to;
    int clockwise = 0;
    switch (f->direction) {
        case DIR_COUNTERCLOCKWISE:
            from = mid - M_PI_2 + M_PI * f->p;
            to = mid + M_PI_2;
            break;
        case DIR_BOTH:
            from = mid - M_PI_2 * left;
            to = mid + M_PI_2 * left;
            break;
        default:
            from = mid + M_PI_2 - M_PI * f->p;
            to = mid - M_PI_2;
            clockwise = 1;
    }
    fill_accent(c, 0.16);
    CGContextMoveToPoint(c, o.x, o.y);
    CGContextAddArc(c, o.x, o.y, radius, from, to, clockwise);
    CGContextClosePath(c);
    CGContextFillPath(c);
    glow(c, 12, 0.9);
    stroke_accent(c, 1);
    CGContextAddArc(c, o.x, o.y, radius, from, to, clockwise);
    CGContextStrokePath(c);
}

static void edge_bar(const EdgeFrame *f) {
    CGContextRef c = f->c;
    const double length = 260, inset = 5;
    double low = -length / 2, high = length / 2;
    // Clockwise eats the bar from the top, or from the left on a horizontal edge.
    bool vertical = f->along.dy != 0;
    bool from_high = vertical == (f->direction == DIR_CLOCKWISE);
    if (f->direction == DIR_BOTH) {
        low += length * f->p / 2;
        high -= length * f->p / 2;
    } else if (from_high) {
        high -= length * f->p;
    } else {
        low += length * f->p;
    }
    CGContextSetLineCap(c, kCGLineCapRound);
    CGContextSetLineWidth(c, 6);
    stroke_accent(c, 0.16);
    CGPoint a = from_anchor(f, -length / 2, inset), b = from_anchor(f, length / 2, inset);
    CGContextMoveToPoint(c, a.x, a.y);
    CGContextAddLineToPoint(c, b.x, b.y);
    CGContextStrokePath(c);
    if (high - low <= 0.5) return;
    glow(c, 14, 1);
    stroke_accent(c, 1);
    a = from_anchor(f, low, inset);
    b = from_anchor(f, high, inset);
    CGContextMoveToPoint(c, a.x, a.y);
    CGContextAddLineToPoint(c, b.x, b.y);
    CGContextStrokePath(c);
}

static void edge_pulse(const EdgeFrame *f) {
    CGContextRef c = f->c;
    // The last fifth of the wait pulls the waves in to a final ping.
    double contract = smooth(0.8, 1.0, f->p);
    double reach = 130 * (1 - contract);
    glow(c, 8, 0.7);
    CGContextSetLineWidth(c, 2.5);
    for (int i = 0; i < 3; i++) {
        double phase = fmod(f->t * 1.4 + i / 3.0, 1.0);
        stroke_accent(c, (1 - phase) * 0.9 * (1 - contract * 0.5));
        circle_path(c, f->anchor, phase * reach);
        CGContextStrokePath(c);
    }
    double core = (8 + 3 * sin(f->t * 10)) * (1 - contract) + 16 * sin(M_PI * contract) * (1 - contract);
    if (core <= 0.05) return;
    glow(c, 16, 1);
    fill_accent(c, 1);
    circle_path(c, f->anchor, core);
    CGContextFillPath(c);
}

static void edge_laser(const EdgeFrame *f) {
    CGContextRef c = f->c;
    const double inset = 3;
    double here = f->anchor.x * f->along.dx + f->anchor.y * f->along.dy;
    double total = f->size.width * f->along.dx + f->size.height * f->along.dy;
    // The beams start at the display's ends whatever the size.
    double ends[2] = { -here / f->zoom, (total - here) / f->zoom };
    double closing = f->p * f->p * (3 - 2 * f->p);
    double fade = 1 - smooth(0.95, 1.0, f->p);
    int frame = (int)floor(f->t * 60);
    CGContextSetLineCap(c, kCGLineCapRound);
    glow(c, 12, 0.9);
    for (int side = 0; side < 2 && fade > 0; side++) {
        double head = ends[side] * (1 - closing);
        double tail = fmin(240, fabs(ends[side] - head)) * (ends[side] < 0 ? -1 : 1);
        const int steps = 14;
        for (int k = 0; k < steps; k++) {
            double a0 = head + tail * k / steps, a1 = head + tail * (k + 1) / steps;
            double jitter = (noise(frame + k * 7 + side) - 0.5) * 2.4;
            CGPoint p0 = from_anchor(f, a0, inset + jitter), p1 = from_anchor(f, a1, inset + jitter);
            stroke_accent(c, pow(1 - (double)k / steps, 1.5) * fade);
            CGContextSetLineWidth(c, 3.5);
            CGContextMoveToPoint(c, p0.x, p0.y);
            CGContextAddLineToPoint(c, p1.x, p1.y);
            CGContextStrokePath(c);
        }
    }
    double burst = smooth(0.84, 1.0, f->p);
    if (burst <= 0 || burst >= 1) return;
    no_glow(c);
    glow(c, 6, 1);
    CGContextSetLineWidth(c, 1.6);
    double mid = atan2(f->inward.dy, f->inward.dx);
    stroke_accent(c, 1 - burst);
    for (int i = 0; i < 18; i++) {
        double angle = mid + (noise(i * 3) - 0.5) * M_PI * 0.95;
        double r0 = 8 + 70 * burst * noise(i * 5), r1 = r0 + 12 + 26 * noise(i * 7);
        CGContextMoveToPoint(c, f->anchor.x + cos(angle) * r0, f->anchor.y + sin(angle) * r0);
        CGContextAddLineToPoint(c, f->anchor.x + cos(angle) * r1, f->anchor.y + sin(angle) * r1);
    }
    CGContextStrokePath(c);
}

static void edge_vortex(const EdgeFrame *f) {
    CGContextRef c = f->c;
    CGPoint o = from_anchor(f, 0, 72);
    double closing = pow(f->p, 1.6);
    double fade = 1 - smooth(0.92, 1.0, f->p);
    if (fade <= 0) return;
    glow(c, 6, 0.8);
    for (int i = 0; i < 56; i++) {
        double speed = (3 + 11 * f->p) * (0.7 + 0.6 * noise(i + 100));
        double angle = noise(i) * 2 * M_PI + f->t * speed;
        double radius = (24 + 58 * noise(i + 200)) * (1 - closing);
        double size = (1.2 + 2.4 * noise(i + 300)) * (1 - f->p * 0.5);
        fill_accent(c, (0.35 + 0.65 * noise(i + 400)) * fade);
        circle_path(c, CGPointMake(o.x + cos(angle) * radius, o.y + sin(angle) * radius), size);
        CGContextFillPath(c);
    }
    glow(c, 18, 1);
    fill_accent(c, fade);
    circle_path(c, o, (3 + 13 * f->p) * fade);
    CGContextFillPath(c);
}

static void edge_scan(const EdgeFrame *f) {
    CGContextRef c = f->c;
    CGPoint o = from_anchor(f, 0, 72);
    double radius = 46 * (1 - f->p * 0.25);
    double alpha = 1 - smooth(0.96, 1.0, f->p);
    // The closer to the end, the faster and harder it flickers.
    double unrest = smooth(0.55, 1.0, f->p);
    if (unrest > 0 && fmod(f->t * (18 + 60 * unrest), 1.0) < 0.1 + 0.4 * unrest) alpha *= 0.25;
    if (alpha <= 0) return;

    glow(c, 8, 0.8);
    CGContextSetLineWidth(c, 2);
    stroke_accent(c, alpha * 0.9);
    for (int i = 0; i < 36; i++) {
        double angle = i * M_PI / 18 + f->t * 0.8;
        double inner = radius - (i % 3 == 0 ? 9 : 4);
        CGContextMoveToPoint(c, o.x + cos(angle) * inner, o.y + sin(angle) * inner);
        CGContextAddLineToPoint(c, o.x + cos(angle) * radius, o.y + sin(angle) * radius);
    }
    CGContextStrokePath(c);

    double sweep = f->t * (4 + 18 * f->p * f->p);
    CGContextSetLineWidth(c, 2.4);
    for (int k = 0; k < 12; k++) {
        double angle = sweep - k * 0.09;
        stroke_accent(c, alpha * pow(1 - k / 12.0, 2));
        CGContextMoveToPoint(c, o.x, o.y);
        CGContextAddLineToPoint(c, o.x + cos(angle) * (radius - 3), o.y + sin(angle) * (radius - 3));
        CGContextStrokePath(c);
    }

    // Torn horizontal slices near the end.
    if (unrest > 0.25) {
        int frame = (int)floor(f->t * 30);
        no_glow(c);
        fill_accent(c, alpha * 0.8);
        for (int i = 0; i < 5; i++) {
            double y = o.y + (noise(frame + i * 11) - 0.5) * radius * 2;
            double x = o.x - radius + noise(frame + i * 13) * radius * 0.6 + (noise(frame + i * 17) - 0.5) * 22 * unrest;
            CGContextFillRect(c, CGRectMake(x, y, radius * (0.6 + noise(frame + i * 19)), 2 + 3 * noise(frame + i * 23)));
        }
    }
}

static void edge_liquid(const EdgeFrame *f) {
    CGContextRef c = f->c;
    const double radius = 58;
    CGPoint o = f->anchor;
    double mid = atan2(f->inward.dy, f->inward.dx);
    double level = radius * (1 - f->p);

    CGContextSetLineWidth(c, 2.5);
    stroke_accent(c, 0.45 * (1 - smooth(0.9, 1.0, f->p)));
    CGContextAddArc(c, o.x, o.y, radius, mid - M_PI_2, mid + M_PI_2, 0);
    CGContextClosePath(c);
    CGContextStrokePath(c);

    if (level <= 0.3) return;
    CGContextSaveGState(c);
    CGContextMoveToPoint(c, o.x, o.y);
    CGContextAddArc(c, o.x, o.y, radius - 2, mid - M_PI_2, mid + M_PI_2, 0);
    CGContextClosePath(c);
    CGContextClip(c);
    CGPoint start = from_anchor(f, -radius, 0);
    CGContextMoveToPoint(c, start.x, start.y);
    for (double a = -radius; a <= radius; a += 3) {
        double wave = sin(a * 0.13 + f->t * 7) * 3 * fmin(1.0, level / 8);
        CGPoint q = from_anchor(f, a, level + wave);
        CGContextAddLineToPoint(c, q.x, q.y);
    }
    CGPoint end = from_anchor(f, radius, 0);
    CGContextAddLineToPoint(c, end.x, end.y);
    CGContextClosePath(c);
    fill_accent(c, 0.9);
    CGContextFillPath(c);
    CGContextRestoreGState(c);
}

// In the order of EdgeAnimStyle in config.rs; its first variant, None, draws nothing.
static const EdgeRenderer kEdgeRenderers[] = {
    NULL, edge_ring, edge_semicircle, edge_bar, edge_pulse, edge_laser, edge_vortex, edge_scan, edge_liquid,
};

// ---- Locate-the-cursor effects ---------------------------------------------------

typedef struct {
    CGContextRef c;
    CGSize size;
    CGPoint center;
    /// Seconds since the effect began.
    double t;
    /// 1 until the fade-out, then down to 0.
    double fade;
    /// The chosen size, for the spotlight; the other effects are scaled as a whole.
    double scale;
} LocateFrame;

typedef void (*LocateRenderer)(const LocateFrame *);

static void locate_ring(const LocateFrame *f) {
    CGContextRef c = f->c;
    glow(c, 14, f->fade);
    CGContextSetLineWidth(c, 5);
    for (int i = 0; i < 3; i++) {
        double phase = clamp01((f->t - i * 0.16) / 0.85);
        if (phase <= 0) continue;
        double eased = 1 - pow(1 - phase, 3);
        stroke_accent(c, (1 - phase * 0.75) * f->fade);
        circle_path(c, f->center, 170 * (1 - eased) + 26);
        CGContextStrokePath(c);
    }
}

static void locate_radar(const LocateFrame *f) {
    CGContextRef c = f->c;
    const double reach = 110;
    glow(c, 8, 0.8 * f->fade);
    CGContextSetLineWidth(c, 1.6);
    for (int i = 1; i <= 3; i++) {
        stroke_accent(c, 0.45 * f->fade);
        circle_path(c, f->center, reach * i / 3);
        CGContextStrokePath(c);
    }
    double sweep = M_PI_2 - f->t * 7;
    CGContextSetLineWidth(c, 2.6);
    for (int k = 0; k < 26; k++) {
        double angle = sweep + k * 0.05;
        stroke_accent(c, pow(1 - k / 26.0, 2) * f->fade);
        CGContextMoveToPoint(c, f->center.x, f->center.y);
        CGContextAddLineToPoint(c, f->center.x + cos(angle) * reach, f->center.y + sin(angle) * reach);
        CGContextStrokePath(c);
    }
    double ping = fmod(f->t * 1.6, 1.0);
    stroke_accent(c, (1 - ping) * f->fade);
    CGContextSetLineWidth(c, 3.5);
    circle_path(c, f->center, ping * reach);
    CGContextStrokePath(c);
}

static void locate_spotlight(const LocateFrame *f) {
    CGContextRef c = f->c;
    double closing = 1 - pow(1 - clamp01(f->t / 0.4), 3);
    // Only the lit circle follows the size: the darkening always covers the display.
    double radius = (600 - 490 * closing) * f->scale;
    double dim = 0.5 * clamp01(f->t / 0.2) * f->fade;
    CGContextSetRGBFillColor(c, 0, 0, 0, dim);
    CGContextAddRect(c, CGRectMake(0, 0, f->size.width, f->size.height));
    circle_path(c, f->center, radius);
    CGContextEOFillPath(c);
    glow(c, 18, f->fade);
    CGContextSetLineWidth(c, 4 * f->scale);
    stroke_accent(c, f->fade);
    circle_path(c, f->center, radius);
    CGContextStrokePath(c);
}

// In the order of LocateStyle in config.rs.
static const LocateRenderer kLocateRenderers[] = { locate_ring, locate_radar, locate_spotlight };

// ---- Windows ---------------------------------------------------------------------

@interface FsOverlayPanel : NSPanel
@end

@implementation FsOverlayPanel
- (BOOL)canBecomeKeyWindow { return NO; }
- (BOOL)canBecomeMainWindow { return NO; }
// May reach past the screen; the effect is simply cut off there.
- (NSRect)constrainFrameRect:(NSRect)frame toScreen:(NSScreen *)screen { (void)screen; return frame; }
@end

@interface FsOverlayView : NSView
@property (nonatomic, copy) void (^painter)(CGContextRef, CGSize);
@end

@implementation FsOverlayView
- (BOOL)isOpaque { return NO; }
- (void)drawRect:(NSRect)dirty {
    CGContextRef c = NSGraphicsContext.currentContext.CGContext;
    CGContextClearRect(c, NSRectToCGRect(dirty));
    if (self.painter) self.painter(c, NSSizeToCGSize(self.bounds.size));
}
@end

static FsOverlayPanel *make_panel(void) {
    FsOverlayPanel *panel = [[FsOverlayPanel alloc] initWithContentRect:NSMakeRect(0, 0, 10, 10)
                                                              styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                                                backing:NSBackingStoreBuffered
                                                                  defer:NO];
    panel.opaque = NO;
    panel.backgroundColor = NSColor.clearColor;
    panel.hasShadow = NO;
    panel.ignoresMouseEvents = YES;
    panel.hidesOnDeactivate = NO;
    panel.releasedWhenClosed = NO;
    panel.level = NSScreenSaverWindowLevel;
    panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorStationary |
                               NSWindowCollectionBehaviorFullScreenAuxiliary | NSWindowCollectionBehaviorIgnoresCycle;
    FsOverlayView *view = [[FsOverlayView alloc] initWithFrame:NSMakeRect(0, 0, 10, 10)];
    view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    panel.contentView = view;
    return panel;
}

static NSTimer *gTimer;

static FsOverlayPanel *gEdgePanel;
static FsOverlayView *gEdgeView;
static struct {
    bool active, preview;
    int edge, style, direction;
    bool shrink;
    double start, duration;
    CGFloat color[3];
    CGFloat scale;
} gEdge;

static FsOverlayPanel *gLocatePanel;
static FsOverlayView *gLocateView;
static struct {
    bool active, full_screen;
    int style;
    double start;
    CGFloat color[3];
    CGFloat scale;
} gLocate;

static NSScreen *screen_at(NSPoint point) {
    for (NSScreen *s in NSScreen.screens) {
        if (NSMouseInRect(point, s.frame, NO)) return s;
    }
    return NSScreen.mainScreen;
}

static void end_edge(void) {
    gEdge.active = false;
    [gEdgePanel orderOut:nil];
}

static void end_locate(void) {
    gLocate.active = false;
    [gLocatePanel orderOut:nil];
}

static void tick(void) {
    double now = CACurrentMediaTime();
    if (gEdge.active) {
        if (gEdge.preview && now - gEdge.start >= gEdge.duration) {
            end_edge();
        } else {
            gEdgeView.needsDisplay = YES;
        }
    }
    if (gLocate.active) {
        if (now - gLocate.start >= kLocateSeconds) {
            end_locate();
        } else {
            if (!gLocate.full_screen) {
                NSPoint m = NSEvent.mouseLocation;
                CGFloat side = kLocateSide * gLocate.scale;
                [gLocatePanel setFrameOrigin:NSMakePoint(m.x - side / 2, m.y - side / 2)];
            }
            gLocateView.needsDisplay = YES;
        }
    }
    if (!gEdge.active && !gLocate.active) {
        [gTimer invalidate];
        gTimer = nil;
    }
}

static void ensure_timer(void) {
    if (gTimer) return;
    gTimer = [NSTimer timerWithTimeInterval:1.0 / 60 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        tick();
    }];
    [NSRunLoop.mainRunLoop addTimer:gTimer forMode:NSRunLoopCommonModes];
}

static void paint_edge(CGContextRef c, CGSize size) {
    if (!gEdge.active) return;
    EdgeRenderer render = kEdgeRenderers[gEdge.style];
    if (!render) return;
    NSPoint m = NSEvent.mouseLocation;
    NSPoint origin = gEdgePanel.frame.origin;
    CGPoint local = CGPointMake(m.x - origin.x, m.y - origin.y);
    EdgeFrame f = { .c = c, .size = size, .direction = gEdge.direction, .shrink = gEdge.shrink, .zoom = gEdge.scale };
    switch (gEdge.edge) {
        case EDGE_LEFT:
            f.anchor = CGPointMake(0, local.y);
            f.inward = CGVectorMake(1, 0);
            f.along = CGVectorMake(0, 1);
            break;
        case EDGE_RIGHT:
            f.anchor = CGPointMake(size.width, local.y);
            f.inward = CGVectorMake(-1, 0);
            f.along = CGVectorMake(0, 1);
            break;
        case EDGE_TOP:
            f.anchor = CGPointMake(local.x, size.height);
            f.inward = CGVectorMake(0, -1);
            f.along = CGVectorMake(1, 0);
            break;
        default:
            f.anchor = CGPointMake(local.x, 0);
            f.inward = CGVectorMake(0, 1);
            f.along = CGVectorMake(1, 0);
    }
    f.t = CACurrentMediaTime() - gEdge.start;
    f.p = clamp01(f.t / gEdge.duration);
    memcpy(gAccent, gEdge.color, sizeof gAccent);
    gGlowScale = gEdge.scale;
    CGContextSaveGState(c);
    // The chosen size: everything grows or shrinks around the point at the edge.
    CGContextTranslateCTM(c, f.anchor.x, f.anchor.y);
    CGContextScaleCTM(c, gEdge.scale, gEdge.scale);
    CGContextTranslateCTM(c, -f.anchor.x, -f.anchor.y);
    render(&f);
    CGContextRestoreGState(c);
}

static void paint_locate(CGContextRef c, CGSize size) {
    if (!gLocate.active) return;
    double t = CACurrentMediaTime() - gLocate.start;
    CGPoint center = CGPointMake(size.width / 2, size.height / 2);
    if (gLocate.full_screen) {
        NSPoint m = NSEvent.mouseLocation;
        NSPoint origin = gLocatePanel.frame.origin;
        center = CGPointMake(m.x - origin.x, m.y - origin.y);
    }
    LocateFrame f = { .c = c, .size = size, .center = center, .t = t, .scale = gLocate.full_screen ? gLocate.scale : 1 };
    f.fade = clamp01((kLocateSeconds - t) / kLocateFade);
    memcpy(gAccent, gLocate.color, sizeof gAccent);
    gGlowScale = gLocate.scale;
    CGContextSaveGState(c);
    if (!gLocate.full_screen) {
        CGContextTranslateCTM(c, center.x, center.y);
        CGContextScaleCTM(c, gLocate.scale, gLocate.scale);
        CGContextTranslateCTM(c, -center.x, -center.y);
    }
    kLocateRenderers[gLocate.style](&f);
    CGContextRestoreGState(c);
}

// ---- Entry points ----------------------------------------------------------------

/// Starts the countdown on one edge of a display. The display is given in CoreGraphics'
/// global coordinates (y down from the top of the main display). `elapsed` is how much of
/// the wait has already passed. A preview takes itself away at the end. The colour is
/// red, green and blue from 0 to 1.
void fs_overlay_edge_begin(double x, double y, double w, double h, int edge, int style, int direction, bool shrink,
                           double elapsed, double duration, bool preview, double red, double green, double blue,
                           double scale) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (style <= 0 || style >= (int)(sizeof kEdgeRenderers / sizeof *kEdgeRenderers)) return;
        if (!gEdgePanel) {
            gEdgePanel = make_panel();
            gEdgeView = (FsOverlayView *)gEdgePanel.contentView;
            gEdgeView.painter = ^(CGContextRef c, CGSize size) { paint_edge(c, size); };
        }
        CGFloat primary = NSScreen.screens.firstObject.frame.size.height;
        NSRect d = NSMakeRect(x, primary - (y + h), w, h);
        NSRect strip = d;
        switch (edge) {
            case EDGE_LEFT: strip.size.width = kStripDepth; break;
            case EDGE_RIGHT: strip.origin.x = NSMaxX(d) - kStripDepth; strip.size.width = kStripDepth; break;
            case EDGE_TOP: strip.origin.y = NSMaxY(d) - kStripDepth; strip.size.height = kStripDepth; break;
            default: strip.size.height = kStripDepth;
        }
        gEdge.active = true;
        gEdge.preview = preview;
        gEdge.edge = edge;
        gEdge.style = style;
        gEdge.direction = direction;
        gEdge.shrink = shrink;
        gEdge.duration = duration > 0.05 ? duration : 0.05;
        gEdge.start = CACurrentMediaTime() - elapsed;
        gEdge.color[0] = red;
        gEdge.color[1] = green;
        gEdge.color[2] = blue;
        gEdge.scale = scale > 0.1 ? scale : 1;
        [gEdgePanel setFrame:strip display:NO];
        gEdgeView.needsDisplay = YES;
        [gEdgePanel orderFrontRegardless];
        ensure_timer();
    });
}

/// Takes the countdown away at once: finished, or called off.
void fs_overlay_edge_end(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (gEdge.active) end_edge();
    });
}

/// Shows where the cursor is, for a moment.
void fs_overlay_locate(int style, double red, double green, double blue, double scale) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (style < 0 || style >= (int)(sizeof kLocateRenderers / sizeof *kLocateRenderers)) return;
        if (!gLocatePanel) {
            gLocatePanel = make_panel();
            gLocateView = (FsOverlayView *)gLocatePanel.contentView;
            gLocateView.painter = ^(CGContextRef c, CGSize size) { paint_locate(c, size); };
        }
        NSPoint m = NSEvent.mouseLocation;
        // Only the spotlight darkens the whole display; the others follow the cursor.
        bool full_screen = kLocateRenderers[style] == locate_spotlight;
        CGFloat side = kLocateSide * (scale > 0.1 ? scale : 1);
        NSRect frame = full_screen ? screen_at(m).frame
                                   : NSMakeRect(m.x - side / 2, m.y - side / 2, side, side);
        gLocate.active = true;
        gLocate.full_screen = full_screen;
        gLocate.style = style;
        gLocate.start = CACurrentMediaTime();
        gLocate.color[0] = red;
        gLocate.color[1] = green;
        gLocate.color[2] = blue;
        gLocate.scale = scale > 0.1 ? scale : 1;
        [gLocatePanel setFrame:frame display:NO];
        gLocateView.needsDisplay = YES;
        [gLocatePanel orderFrontRegardless];
        ensure_timer();
    });
}
