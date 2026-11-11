import Foundation
import Testing
@testable import SheprdRemoteCore

struct StandaloneGitHubTests {
    @Test func capabilityDistinguishesOlderDisabledAndEnabledHosts() throws {
        let base = #"""
        {"computer":"Cloud","fingerprint":"fixture","connected":true,
         "waiting":0,"workspaces":[]}
        """#
        let older = try JSONDecoder().decode(Home.self, from: Data(base.utf8))
        #expect(older.github == nil)
        for enabled in [false, true] {
            var json = try #require(JSONSerialization.jsonObject(with: Data(base.utf8)) as? [String: Any])
            json["github"] = enabled
            let home = try JSONDecoder().decode(Home.self, from: JSONSerialization.data(withJSONObject: json))
            #expect(home.github == enabled)
            let roundtrip = try JSONDecoder().decode(Home.self, from: JSONEncoder().encode(home))
            #expect(roundtrip.github == enabled)
        }
    }
}
