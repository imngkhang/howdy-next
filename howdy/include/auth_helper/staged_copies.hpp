#pragma once

#include <ostream>
#include <string>

namespace howdy::native::auth_helper {

	// Removes the copies of a user's face models that howdy-auth-helper staged for unprivileged
	// PAM consumers, so removed models do not stay readable until the next verification.
	// Best effort for commands that just changed the source models: anything it could not remove
	// is reported on `output`, and it never fails the caller. Does nothing unless run as root.
	void PurgeStagedCopiesForUser(const std::string &user, std::ostream &output);

}  // namespace howdy::native::auth_helper
