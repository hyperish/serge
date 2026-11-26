"""Configure both Pixel Fold displays and its hinge on the runner's API 35 AVD."""
from pathlib import Path
import sys


def configure(path):
    # The API 35 image uses the cover display to select the Pixel Fold
    # device-state and WindowManager overlays at boot.
    hardware = {
        "hw.device.name": "pixel_fold", "hw.device.manufacturer": "Google",
        "hw.lcd.width": "2208", "hw.lcd.height": "1840", "hw.lcd.density": "420",
        "skin.name": "2208x1840", "skin.path": "2208x1840",
        "hw.display1.width": "1080", "hw.display1.height": "2092", "hw.display1.density": "420",
        "hw.display1.flag": "0",
        "hw.displayRegion.0.1.xOffset": "0", "hw.displayRegion.0.1.yOffset": "0",
        "hw.displayRegion.0.1.width": "1080", "hw.displayRegion.0.1.height": "2092",
        "hw.sensor.hinge": "yes", "hw.sensor.hinge.count": "1",
        "hw.sensor.hinge.type": "1", "hw.sensor.hinge.sub_type": "1",
        "hw.sensor.hinge.ranges": "0-180", "hw.sensor.hinge.defaults": "180",
        "hw.sensor.hinge.areas": "1080-0-0-1840",
        "hw.sensor.posture_list": "1,2,3",
        "hw.sensor.hinge_angles_posture_definitions": "0-30,30-150,150-180",
    }
    original = path.read_text().splitlines()
    lines = [line for line in original if line.partition("=")[0].strip() not in hardware]
    lines.extend(f"{key}={value}" for key, value in hardware.items())
    path.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    configure(Path(sys.argv[1]))
