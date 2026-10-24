import os, sys, unittest
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import profiles


class Profiles(unittest.TestCase):
    def test_inference_offers_nothing(self):
        kw = profiles.option_kwargs("inference", cwd="/c")
        self.assertEqual(kw["tools"], [])
        self.assertEqual(kw["allowed_tools"], [])
        self.assertEqual(kw["disallowed_tools"], ["*"])
        self.assertEqual(profiles.allowed_api_tools("inference"), frozenset())

    def test_every_profile_closes_the_hidden_paths_the_probe_found(self):
        for p in profiles.PROFILES:
            kw = profiles.option_kwargs(p, cwd="/c")
            self.assertEqual(kw["tools"], [], p)
            self.assertEqual(kw["setting_sources"], [], p)
            self.assertTrue(kw["strict_mcp_config"], p)
            self.assertTrue(kw["verbatim_prompts"], p)
            self.assertEqual(kw["plugins"], [], p)
            self.assertEqual(kw["skills"], [], p)
            self.assertEqual(kw["permission_mode"], "dontAsk", p)
            for flag in ("bare", "safe-mode", "disable-slash-commands"):
                self.assertIn(flag, kw["extra_args"], p)
            self.assertEqual(kw["extra_args"]["permission-prompts"], "none")

    def test_local_tools_allows_only_authority_tools(self):
        kw = profiles.option_kwargs("local_tools", cwd="/c")
        self.assertEqual(kw["allowed_tools"], list(profiles.AUTHORITY_TOOL_NAMES))
        self.assertTrue(all(n.startswith("mcp__wyss__") for n in kw["allowed_tools"]))
        self.assertEqual(profiles.allowed_api_tools("local_tools"), frozenset(profiles.AUTHORITY_TOOL_NAMES) | frozenset(profiles.INERT_TOOL_NAMES))

    def test_unknown_profile_and_optional_fields(self):
        with self.assertRaises(ValueError):
            profiles.option_kwargs("bypass", cwd="/c")
        with self.assertRaises(ValueError):
            profiles.allowed_api_tools("bypass")
        kw = profiles.option_kwargs("inference", cwd="/c")
        self.assertNotIn("model", kw)
        self.assertNotIn("effort", kw)
        self.assertNotIn("resume", kw)
        kw = profiles.option_kwargs("inference", cwd="/c", model="m", effort="low", resume="r")
        self.assertEqual((kw["model"], kw["effort"], kw["resume"]), ("m", "low", "r"))


if __name__ == "__main__":
    unittest.main()
