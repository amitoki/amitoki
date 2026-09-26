"""公式イメージの検証と、ゲストごとのcloud-init seed作成。"""

import hashlib
import json
import os
import secrets
import shutil
import subprocess

from settings import CACHE, DISK_SIZE, IMAGE_NAME, IMAGE_URL, STATE


def prepare_image():
    CACHE.mkdir(parents=True, exist_ok=True)
    image = CACHE / IMAGE_NAME
    checksums = CACHE / "SHA256SUMS"
    if not checksums.exists():
        subprocess.run(["curl", "-fLsS", f"{IMAGE_URL}/SHA256SUMS", "-o", str(checksums)], check=True)
    checksum = next(line.split()[0] for line in checksums.read_text().splitlines()
                    if line.split()[-1].lstrip("*") == IMAGE_NAME)
    if not image.exists():
        partial = image.with_suffix(".img.part")
        subprocess.run(["curl", "-fL", f"{IMAGE_URL}/{IMAGE_NAME}", "-o", str(partial)], check=True)
        partial.rename(image)
    with image.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != checksum:
        raise RuntimeError(f"イメージのSHA256が一致しません: {image}")
    return image


def prepare_credentials():
    STATE.mkdir(mode=0o700, parents=True, exist_ok=True)
    STATE.chmod(0o700)
    key = STATE / "id_ed25519"
    if not key.exists():
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True)
    password_file = STATE / "postgres-password"
    if not password_file.exists():
        password_file.write_text(secrets.token_hex(24))
        password_file.chmod(0o600)
    return password_file.read_text().strip()


def seed_guest(node, image):
    directory = STATE / node
    directory.mkdir(exist_ok=True)
    disk = directory / "disk.qcow2"
    if disk.exists():
        return
    iso_tool = os.environ.get("AMITOKI_GENISOIMAGE", "genisoimage")
    if not shutil.which(iso_tool):
        raise RuntimeError("genisoimageが必要です。docs/vm-lab.mdの準備手順を参照してください")
    packages = ["iproute2", "iputils-ping", "python3", "ethtool"]
    if node == "a":
        packages.append("postgresql")
    configuration = {
        "hostname": f"amitoki-{node}", "manage_etc_hosts": True,
        "ssh_pwauth": False, "ssh_authorized_keys": [(STATE / "id_ed25519.pub").read_text().strip()],
        "package_update": True, "packages": packages,
    }
    (directory / "user-data").write_text("#cloud-config\n" + json.dumps(configuration))
    (directory / "meta-data").write_text(f"instance-id: amitoki-{node}\nlocal-hostname: amitoki-{node}\n")
    subprocess.run([
        iso_tool, "-quiet", "-output", str(directory / "seed.iso"), "-volid", "cidata",
        "-joliet", "-rock", str(directory / "user-data"), str(directory / "meta-data"),
    ], check=True)
    subprocess.run(["qemu-img", "create", "-f", "qcow2", "-F", "qcow2", "-b", str(image), str(disk), DISK_SIZE], check=True)
