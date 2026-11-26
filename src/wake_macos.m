// Tells the Rust side when the Mac wakes from sleep or the screen is unlocked, so the
// network can look for the other computers again at once.

#import <AppKit/AppKit.h>

static void (*gCallback)(void);

static void notify(void) {
    if (gCallback) gCallback();
}

/// Calls `callback` (on the main thread) at every wake-up and every unlock. Observers are
/// added once; calling this again only replaces the callback.
void fs_wake_observe(void (*callback)(void)) {
    gCallback = callback;
    dispatch_async(dispatch_get_main_queue(), ^{
        static BOOL started;
        if (started) return;
        started = YES;
        [NSWorkspace.sharedWorkspace.notificationCenter addObserverForName:NSWorkspaceDidWakeNotification
                                                                     object:nil
                                                                      queue:nil
                                                                 usingBlock:^(NSNotification *note) {
            (void)note;
            notify();
        }];
        // Posted by the login window when the screen is unlocked.
        [NSDistributedNotificationCenter.defaultCenter addObserverForName:@"com.apple.screenIsUnlocked"
                                                                    object:nil
                                                                     queue:nil
                                                                usingBlock:^(NSNotification *note) {
            (void)note;
            notify();
        }];
    });
}
