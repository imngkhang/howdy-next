#include "auth_helper/staged_copies.hpp"

#include "auth_helper/runtime.hpp"
#include "support/passwd_lookup.hpp"

#include <ostream>
#include <string>

#include <unistd.h>

namespace howdy::native::auth_helper {

	void PurgeStagedCopiesForUser(const std::string &user, std::ostream &output) {
		// Only root can change the runtime root, and the staged copies are keyed by UID.
		if (geteuid() != 0) {
			return;
		}
		const auto account = LookupPasswdByName(user);
		if (!account.has_value()) {
			output << "Could not resolve " << user << " to a UID; any staged copy of its face "
			       << "models under " << RuntimeRoot().string() << " stays until reboot.\n";
			return;
		}

		switch (PurgeRuntimeAuthFiles({.uid = account->uid})) {
			case StagedPurgeResult::kPurged:
			case StagedPurgeResult::kNothingStaged:
				return;
			case StagedPurgeResult::kInUse:
				output << "A staged copy of the face models for " << user
				       << " is in use; the next face verification replaces it.\n";
				return;
			case StagedPurgeResult::kFailed:
				output << "Could not remove the staged copy of the face models for " << user
				       << " under " << RuntimeRoot().string()
				       << "; the next face verification replaces it.\n";
				return;
		}
	}

}  // namespace howdy::native::auth_helper
