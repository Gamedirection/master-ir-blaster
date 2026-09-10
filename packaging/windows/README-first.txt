IR Blaster for Windows - first-time setup
==========================================

Before the app can see the Tiqiaa TView USB IR transceiver (VID 10C4,
PID 8468), Windows needs a WinUSB driver bound to it. This is a one-time
step per machine, similar to the udev rule Linux users install.

1. Plug in the IR transceiver.
2. Download Zadig: https://zadig.akeo.ie
3. Run Zadig, then go to Options > List All Devices.
4. In the device dropdown, carefully select the Tiqiaa/TView device
   (VID_10C4&PID_8468). Do not pick any other device in the list - binding
   the wrong device's driver can break that device.
5. Set the driver to WinUSB and click "Replace Driver" (or "Install
   Driver").
6. Run IR-Blaster.exe.

If the app still reports the device as "not found" after this, unplug and
replug the transceiver, then try again.
