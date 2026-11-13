"""The relay's DigitalOcean user data, rendered the way create-droplet.sh
renders it, picks the domain it was given, or IP.sslip.io without one, and
runs only a relay image whose signature it verified."""

import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CLOUD_INIT = os.path.join(ROOT, "deploy", "relay", "cloud-init.yml")


def rendered(domain: str) -> str:
    """cloud-init.yml after create-droplet.sh's sed substitution."""
    with open(CLOUD_INIT, encoding="utf-8") as file:
        text = file.read()
    return (
        text.replace("__COMPOSE_B64__", "Y29tcG9zZQ==")
        .replace("__CADDYFILE_B64__", "Y2FkZHk=")
        .replace("__RELAY_DOMAIN__", domain)
    )


def chosen_domain(text: str) -> str:
    """Runs start.sh's domain choice, with the metadata service answering
    203.0.113.7, and returns the domain it settles on."""
    block = re.search(r'\n(\s*)domain="[^"\n]*"\n.*?\n\1fi\n', text, re.S)
    assert block, "start.sh's domain block is missing"
    script = "\n".join(line.strip() for line in block.group(0).splitlines())
    shell = 'curl() { echo 203.0.113.7; }\n' + script + '\nprintf "%s" "$domain"\n'
    with tempfile.TemporaryDirectory() as directory:
        path = os.path.join(directory, "domain.sh")
        with open(path, "w", encoding="utf-8") as file:
            file.write(shell)
        return subprocess.run(
            ["sh", path], check=True, capture_output=True, text=True
        ).stdout


def bans_mode(text: str) -> tuple[int, int]:
    """Runs start.sh's ban list setup under the umask start.sh has by then,
    and returns the directory's and the list's permissions."""
    block = re.search(r"\n\s*\(umask 022; mkdir -p bans.*?chmod 644 bans/list\n", text, re.S)
    assert block, "start.sh's ban list block is missing"
    script = "umask 077\n" + "\n".join(line.strip() for line in block.group(0).splitlines())
    with tempfile.TemporaryDirectory() as directory:
        subprocess.run(["sh", "-c", script], cwd=directory, check=True)
        bans = os.path.join(directory, "bans")
        return os.stat(bans).st_mode & 0o777, os.stat(os.path.join(bans, "list")).st_mode & 0o777


IDENTITY = "https://github.com/sheprd-labs/sheprd/.github/workflows/relay-image.yml@refs/heads/main"
ISSUER = "https://token.actions.githubusercontent.com"
DIGEST = "sha256:" + "ab" * 32


def signatures(*digests: str) -> str:
    """A fake cosign that logs its arguments and verifies DIGESTS."""
    payload = json.dumps([{"critical": {"image": {"docker-manifest-digest": d}}} for d in digests])
    return f"echo \"$@\" >&2\nprintf '%s' '{payload}'"


def verified_image(text: str, cosign: str, relay_image: str = "") -> subprocess.CompletedProcess:
    """Runs start.sh's image check with COSIGN as the cosign script's body;
    stdout is the image compose would run."""
    block = re.search(r"\n(\s*)verified_image\(\) \{\n.*?\n\1\}\n", text, re.S)
    assert block, "start.sh's image check is missing"
    script = "\n".join(line.strip() for line in block.group(0).splitlines())
    with tempfile.TemporaryDirectory() as directory:
        fake = os.path.join(directory, "cosign")
        with open(fake, "w", encoding="utf-8") as file:
            file.write("#!/bin/sh\n" + cosign + "\n")
        os.chmod(fake, 0o755)
        shell = f'cosign="{fake}"\n{script}\nimage=$(verified_image) || exit 1\nprintf "%s" "$image"\n'
        environment = {"PATH": os.environ["PATH"], "RELAY_IMAGE": relay_image}
        return subprocess.run(["sh", "-c", shell], capture_output=True, text=True, env=environment)


@unittest.skipUnless(shutil.which("jq"), "jq is not installed")
class ImageTest(unittest.TestCase):
    def test_the_tag_runs_as_the_digest_its_signature_covers(self) -> None:
        result = verified_image(rendered(""), signatures(DIGEST))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"ghcr.io/sheprd-labs/sheprd-relay@{DIGEST}")
        self.assertIn(
            f"verify ghcr.io/sheprd-labs/sheprd-relay:latest --certificate-identity {IDENTITY} "
            f"--certificate-oidc-issuer {ISSUER}",
            result.stderr,
        )

    def test_a_bad_signature_runs_nothing(self) -> None:
        result = verified_image(rendered(""), "exit 1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("the running relay is unchanged", result.stderr)

    def test_signatures_naming_two_digests_run_nothing(self) -> None:
        result = verified_image(rendered(""), signatures(DIGEST, "sha256:" + "cd" * 32))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")

    def test_an_operators_digest_is_used_as_given(self) -> None:
        pinned = f"registry.example.org:5000/relay@{DIGEST}"
        result = verified_image(rendered(""), "exit 1", pinned)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, pinned)

    def test_an_operators_tag_is_verified(self) -> None:
        tagged = "registry.example.org:5000/relay:v2"
        result = verified_image(rendered(""), signatures(DIGEST), tagged)
        self.assertEqual(result.stdout, f"registry.example.org:5000/relay@{DIGEST}")
        self.assertIn(f"verify {tagged} --certificate-identity {IDENTITY}", result.stderr)


class StartTest(unittest.TestCase):
    def test_compose_runs_the_verified_image_without_pulling_tags(self) -> None:
        text = rendered("")
        self.assertNotIn("--pull always", text)
        self.assertIn('RELAY_IMAGE="$image" docker compose --env-file compose.env up -d', text)
        self.assertLess(text.index("image=$(verified_image) || exit 1"), text.index("docker compose"))

    def test_cosign_is_checked_against_a_pinned_hash(self) -> None:
        hashes = re.findall(r"(amd64|arm64)\) cosign_sha256=([0-9a-f]{64}) ;;", rendered(""))
        self.assertEqual({arch for arch, _ in hashes}, {"amd64", "arm64"})


class BansTest(unittest.TestCase):
    def test_the_ban_list_is_readable_by_the_relay(self) -> None:
        self.assertEqual(bans_mode(rendered("")), (0o755, 0o644))


def service_block(path: str, indent: int) -> list[str]:
    """The relay service's lines in an App Platform spec, without their
    indentation, from its `- name: relay` to the end of the file."""
    with open(os.path.join(ROOT, path), encoding="utf-8") as file:
        lines = file.read().splitlines()
    start = next(index for index, line in enumerate(lines) if line.strip() == "- name: relay")
    return [line[indent:] for line in lines[start:] if line.strip()]


class AppPlatformTest(unittest.TestCase):
    def test_the_deploy_button_template_matches_the_doctl_spec(self) -> None:
        self.assertEqual(
            service_block(".do/deploy.template.yaml", 4),
            service_block("deploy/relay/app.yaml", 2),
        )

    def test_the_image_is_the_published_one(self) -> None:
        for path in (".do/deploy.template.yaml", "deploy/relay/app.yaml"):
            with self.subTest(path=path):
                block = "\n".join(service_block(path, 0))
                self.assertIn("registry: ghcr.io", block)
                self.assertIn("repository: sheprd-labs/sheprd-relay", block)


class DomainTest(unittest.TestCase):
    def test_a_given_domain_is_kept(self) -> None:
        self.assertEqual(
            chosen_domain(rendered("nyc3.relay.example.org")), "nyc3.relay.example.org"
        )

    def test_no_domain_falls_back_to_sslip(self) -> None:
        self.assertEqual(chosen_domain(rendered("")), "203-0-113-7.sslip.io")

    def test_an_unfilled_placeholder_falls_back_to_sslip(self) -> None:
        with open(CLOUD_INIT, encoding="utf-8") as file:
            self.assertEqual(chosen_domain(file.read()), "203-0-113-7.sslip.io")


if __name__ == "__main__":
    unittest.main()
