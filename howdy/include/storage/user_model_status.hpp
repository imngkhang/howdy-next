#pragma once

#include <cstdint>
#include <string_view>

namespace howdy::native {
	inline constexpr std::string_view kUserModelsDirectoryLabel = "User models directory";
	inline constexpr std::string_view kUserModelFileLabel       = "User model file";
	inline constexpr std::string_view kUserModelFileInspectionFailedMessage =
	    "Failed to inspect user model file";
	inline constexpr std::string_view kUserModelChangedMessage =
	    "User model file changed, please rerun the command";

	enum class UserModelStatus : std::uint8_t {
		kOk,
		kNoModel,
		kNoModelDirectory,
		kIncompatibleBackend,
		kIncompatibleModel,
		kParseError,
		kInvalidShape,
		kOversized,
		kInsecurePath,
		kInvalidUser,
		kLockFailed,
		kWriteFailed,
		kAtomicExchangeUnsupported,
		kDeleteFailed,
		kDurabilityUncertain,
		kCommitStateUncertain,
		kDirectoryCreateFailed,
		kModelChanged,
	};

}  // namespace howdy::native
