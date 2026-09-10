IR Blaster for macOS - first launch
====================================

This build isn't code-signed or notarized yet, so macOS Gatekeeper will
block a normal double-click the first time with a message like "IR Blaster
is damaged and can't be opened" or "cannot verify the developer".

To open it anyway, use either of these:

Option 1 (Finder):
1. Right-click (or Control-click) IR Blaster.app in Applications.
2. Choose Open.
3. Click Open in the dialog that appears. You only need to do this once -
   after that, it opens normally.

Option 2 (Terminal):
    xattr -cr "/Applications/IR Blaster.app"

Then open it normally.
