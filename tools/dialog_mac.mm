// AppKit dialogs run on the main thread, before Bevy owns the event loop.
#import <AppKit/AppKit.h>
#include <cstdlib>
#include <cstring>

static bool prepare() {
    if (![NSThread isMainThread]) return false;
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    [NSApp activateIgnoringOtherApps:YES];
    return true;
}

extern "C" char* tz_dialog_folder(const char* title, const char* start) {
    @autoreleasepool {
        if (!prepare()) return nullptr;
        NSOpenPanel* panel = [NSOpenPanel openPanel];
        panel.title = [NSString stringWithUTF8String:title];
        panel.canChooseFiles = NO;
        panel.canChooseDirectories = YES;
        panel.allowsMultipleSelection = NO;
        panel.canCreateDirectories = NO;
        if (start) {
            panel.directoryURL = [NSURL fileURLWithFileSystemRepresentation:start
                isDirectory:YES relativeToURL:nil];
        }
        if ([panel runModal] != NSModalResponseOK || !panel.URL) return nullptr;
        const char* path = panel.URL.fileSystemRepresentation;
        return path ? strdup(path) : nullptr;
    }
}

extern "C" void tz_dialog_free(char* path) { free(path); }

extern "C" bool tz_dialog_ask(const char* text, bool warning) {
    @autoreleasepool {
        if (!prepare()) return false;
        NSAlert* alert = [[NSAlert alloc] init];
        alert.messageText = @"ReTzar";
        alert.informativeText = [NSString stringWithUTF8String:text];
        alert.alertStyle = warning ? NSAlertStyleWarning : NSAlertStyleInformational;
        [alert addButtonWithTitle:@"OK"];
        [alert addButtonWithTitle:NSLocalizedString(@"Cancel", nil)];
        return [alert runModal] == NSAlertFirstButtonReturn;
    }
}

extern "C" void tz_dialog_error(const char* text) {
    @autoreleasepool {
        if (!prepare()) return;
        NSAlert* alert = [[NSAlert alloc] init];
        alert.messageText = @"ReTzar";
        alert.informativeText = [NSString stringWithUTF8String:text];
        alert.alertStyle = NSAlertStyleCritical;
        [alert addButtonWithTitle:@"OK"];
        [alert runModal];
    }
}
