#pragma once

#include "app/command_invocation.hpp"
#include "storage/user_model_types.hpp"

#include <string>

namespace howdy::native::remove_internal {

	using ListUserModelEntriesFn = howdy::native::UserModelListResult (*)(void *context,
	                                                                      const std::string &user);

	using RemoveUserModelEntryIfMatchesFn = howdy::native::UserModelMutationResult (*)(
	    void *context, const std::string &user,
	    const howdy::native::UserModelEntryExpectation &expected);

	// Optional: drops staged runtime copies after a model is removed.
	using PurgeStagedCopiesFn = void (*)(void *context, const std::string &user);

	struct RemoveDependencies {
		void                           *context                            = nullptr;
		ListUserModelEntriesFn          list_user_model_entries            = nullptr;
		RemoveUserModelEntryIfMatchesFn remove_user_model_entry_if_matches = nullptr;
		PurgeStagedCopiesFn             purge_staged_copies                = nullptr;
	};

	auto RemoveMainWithDependencies(const CommandInvocation  &invocation,
	                                const RemoveDependencies &dependencies) -> int;

}  // namespace howdy::native::remove_internal
