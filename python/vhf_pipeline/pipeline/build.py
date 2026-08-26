import subprocess


def main():
    subprocess.run(["uv", "run", "cargo", "build", "--release"], check=True)
