#pragma once

#include "storage/user_model_status.hpp"
#include "storage/user_model_types.hpp"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <string_view>

// Binary user-model file codec. The byte layout, limits, and label rules are owned by the Rust
// `howdy-user-model` crate; this wrapper is the only C++ code that reaches it.
namespace howdy::native::user_model_codec {

	// Limits the Rust codec enforces. user_model_codec.cpp static_asserts each one against the
	// generated FFI header, so they cannot drift from Rust (the config size-limit pattern).
	inline constexpr std::size_t kMaxFileBytes         = std::size_t{1024} * 1024;
	inline constexpr std::size_t kMaxModels            = 256;
	inline constexpr std::size_t kMaxEncodingsPerModel = 32;
	inline constexpr std::size_t kMaxEncodingLength    = 1024;
	inline constexpr std::size_t kMaxLabelBytes        = 24;
	// Largest entry id; a stored next id is at most kMaxId + 1, which still fits an int.
	inline constexpr int kMaxId = 2147483646;

	// Why a label is not valid for a new entry (the Rust codec's LabelProblem).
	enum class LabelProblem : std::uint8_t {
		kNone,
		kEmpty,
		kTooLong,
		kMalformed,
		kHidden,
	};

	// The next id to store after adding an entry with `id`, as the Rust codec computes it.
	[[nodiscard]] auto NextIdAfter(int id) -> int;

	struct EncodeResult {
		UserModelStatus status = UserModelStatus::kOk;
		std::string     error_message;
		std::string     content;
	};

	// Decodes model file bytes. Success is kOk with at least one entry and the stored next_id,
	// which is above every id; every rejected file is a failure status with a message. The bytes
	// are released once Rust has decoded them, before the entries are copied out, so a large file
	// is never held three times at once.
	[[nodiscard]] auto Decode(std::string content) -> UserModelListResult;

	// Encodes entries (ids strictly ascending) and next_id (above every id) into model file
	// bytes, re-checked by decoding. Keep next_id when removing entries, so ids are never reused.
	[[nodiscard]] auto Encode(std::span<const UserModelEntry> entries, int next_id) -> EncodeResult;

	// What, if anything, makes label invalid for a new entry. Existing labels are read and
	// re-written under a looser safety rule, so only writers of new labels apply this.
	[[nodiscard]] auto CheckModelLabel(std::string_view label) -> LabelProblem;

	// Whether label is valid for a new entry.
	[[nodiscard]] auto IsValidModelLabel(std::string_view label) -> bool;

	// kOk when label may be stored on a new entry, otherwise the codec's invalid-label failure.
	[[nodiscard]] auto ValidateNewLabel(std::string_view label) -> EncodeResult;

	// A stored label for terminal output, with invisible format and control characters shown as
	// <U+XXXX> so a label written before (or around) the display rule cannot disguise itself.
	[[nodiscard]] auto DisplayModelLabel(std::string_view label) -> std::string;

	// Length of the longest prefix of text that fits kMaxLabelBytes without splitting a UTF-8
	// sequence, or nullopt if the codec could not be reached.
	[[nodiscard]] auto ModelLabelPrefixLength(std::string_view text) -> std::optional<std::size_t>;

}  // namespace howdy::native::user_model_codec
