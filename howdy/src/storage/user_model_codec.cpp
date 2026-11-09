#include "storage/user_model_codec.hpp"

#include "storage/user_model_status.hpp"
#include "storage/user_model_types.hpp"
#include "support/face_metric.hpp"
#include "support/rust_ffi/text.hpp"

#include "howdy_ffi.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <memory>
#include <new>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace howdy::native::user_model_codec {

	static_assert(kMaxFileBytes == HOWDY_USER_MODEL_MAX_FILE_BYTES);
	static_assert(kMaxModels == HOWDY_USER_MODEL_MAX_MODELS);
	static_assert(kMaxEncodingsPerModel == HOWDY_USER_MODEL_MAX_ENCODINGS_PER_MODEL);
	static_assert(kMaxEncodingLength == HOWDY_USER_MODEL_MAX_ENCODING_LEN);
	static_assert(kMaxLabelBytes == HOWDY_USER_MODEL_MAX_LABEL_BYTES);
	static_assert(static_cast<std::uint32_t>(kMaxId) == HOWDY_USER_MODEL_MAX_ID);
	static_assert(kMaxId < std::numeric_limits<int>::max(), "next id kMaxId + 1 must fit an int");

	namespace {

		using rust_ffi::Text;
		using rust_ffi::View;

		struct DecodedDeleter {
			void operator()(HowdyUserModelDecoded *decoded) const noexcept {
				howdy_user_model_decoded_free(decoded);
			}
		};

		struct EncodedDeleter {
			void operator()(HowdyUserModelEncoded *encoded) const noexcept {
				howdy_user_model_encoded_free(encoded);
			}
		};

		using DecodedHandle = std::unique_ptr<HowdyUserModelDecoded, DecodedDeleter>;
		using EncodedHandle = std::unique_ptr<HowdyUserModelEncoded, EncodedDeleter>;

		constexpr std::string_view kDecodeFailedMessage = "Failed to process user model file";
		constexpr std::string_view kEncodeFailedMessage = "Failed to encode user model file";

		struct Failure {
			UserModelStatus  status;
			std::string_view message;
		};

		// The one mapping from codec errors to C++ statuses and user-facing messages.
		auto DescribeError(HowdyUserModelErrorKind kind) -> Failure {
			switch (kind) {
				case HOWDY_USER_MODEL_ERROR_KIND_TOO_LARGE:
					return {
					    .status  = UserModelStatus::kOversized,
					    .message = "User model file exceeds safety limit",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_TRUNCATED:
					return {
					    .status  = UserModelStatus::kParseError,
					    .message = "User model file is truncated",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_BAD_MAGIC:
					return {
					    .status  = UserModelStatus::kParseError,
					    .message = "File is not a Howdy user model file",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_UNSUPPORTED_VERSION:
					return {
					    .status  = UserModelStatus::kParseError,
					    .message = "User model file format version is not supported",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_CHECKSUM_MISMATCH:
					return {
					    .status  = UserModelStatus::kParseError,
					    .message = "User model file checksum does not match; the file is corrupted",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_TRAILING_BYTES:
					return {
					    .status  = UserModelStatus::kParseError,
					    .message = "User model file has unexpected trailing data",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_NO_MODELS:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "User model file contains no models",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_TOO_MANY_MODELS:
					return {
					    .status  = UserModelStatus::kOversized,
					    .message = "Face model list exceeds the safety limit",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_ID_OUT_OF_RANGE:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face model ID is out of range",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_ID_NOT_ASCENDING:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face model IDs must be unique and ascending",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_INVALID_METRIC:
					return {
					    .status  = UserModelStatus::kParseError,
					    .message = "Face model has an unknown face metric",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_INVALID_LABEL:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face model label is invalid",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_INVALID_METADATA:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face model backend or model name is invalid",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_NO_ENCODINGS:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face model contains no encodings",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_TOO_MANY_ENCODINGS:
					return {
					    .status  = UserModelStatus::kOversized,
					    .message = "Face model contains too many encodings",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_ENCODING_LENGTH:
					return {
					    .status  = UserModelStatus::kOversized,
					    .message = "Face encoding length is outside the safety limit",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_ENCODING_LENGTH_MISMATCH:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face encodings differ in length",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_NON_FINITE_VALUE:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face encoding contains a NaN or infinite value",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_INVALID_NEXT_ID:
					return {
					    .status  = UserModelStatus::kInvalidShape,
					    .message = "Face model next ID is invalid",
					};
				case HOWDY_USER_MODEL_ERROR_KIND_ENCODE_MISMATCH:
					return {
					    .status  = UserModelStatus::kWriteFailed,
					    .message = kEncodeFailedMessage,
					};
			}
			return {
			    .status  = UserModelStatus::kParseError,
			    .message = kDecodeFailedMessage,
			};
		}

		auto MetricCode(FaceMetric metric) -> std::uint32_t {
			switch (metric) {
				case FaceMetric::kCosine:
					return HOWDY_USER_MODEL_METRIC_COSINE;
				case FaceMetric::kL2:
					return HOWDY_USER_MODEL_METRIC_L2;
				case FaceMetric::kL2Norm:
					return HOWDY_USER_MODEL_METRIC_L2_NORM;
			}
			return 0;  // Not a metric code; Rust rejects it as an invalid argument.
		}

		auto ToFaceMetric(HowdyUserModelMetric metric) -> std::optional<FaceMetric> {
			switch (metric) {
				case HOWDY_USER_MODEL_METRIC_COSINE:
					return FaceMetric::kCosine;
				case HOWDY_USER_MODEL_METRIC_L2:
					return FaceMetric::kL2;
				case HOWDY_USER_MODEL_METRIC_L2_NORM:
					return FaceMetric::kL2Norm;
			}
			return std::nullopt;
		}

		auto DecodeFailure(UserModelStatus status, std::string_view message)
		    -> UserModelListResult {
			return UserModelListResult{
			    .status        = status,
			    .error_message = std::string(message),
			};
		}

		auto EncodeFailure(UserModelStatus status, std::string_view message) -> EncodeResult {
			return EncodeResult{
			    .status        = status,
			    .error_message = std::string(message),
			};
		}

		auto ReadEntry(const HowdyUserModelDecoded *decoded, std::size_t index)
		    -> std::optional<UserModelEntry> {
			HowdyUserModelEntryView view{.metric = HOWDY_USER_MODEL_METRIC_COSINE};
			if (howdy_user_model_decoded_entry(decoded, index, &view) != HOWDY_FFI_STATUS_OK) {
				return std::nullopt;
			}
			const auto metric = ToFaceMetric(view.metric);
			if (!metric.has_value() || view.values == nullptr) {
				return std::nullopt;
			}
			UserModelEntry entry{
			    .id      = static_cast<int>(view.id),
			    .time    = view.time,
			    .label   = std::string(Text(view.label)),
			    .backend = std::string(Text(view.backend)),
			    .metric  = *metric,
			    .model   = std::string(Text(view.model)),
			};
			const std::span<const float> values(view.values,
			                                    view.encoding_count * view.encoding_len);
			entry.encodings.reserve(view.encoding_count);
			for (std::size_t offset = 0; offset < values.size(); offset += view.encoding_len) {
				const auto encoding = values.subspan(offset, view.encoding_len);
				entry.encodings.emplace_back(encoding.begin(), encoding.end());
			}
			return entry;
		}

		auto DecodeEntries(std::string &content) -> UserModelListResult {
			HowdyUserModelDecoded *raw    = nullptr;
			const auto             status = howdy_user_model_decode(View(content), &raw);
			const DecodedHandle    handle(raw);
			// Rust owns the decoded entries now; drop the file bytes before copying them out.
			std::string().swap(content);
			if (status == HOWDY_FFI_STATUS_TOO_LARGE) {
				return DecodeFailure(UserModelStatus::kOversized,
				                     DescribeError(HOWDY_USER_MODEL_ERROR_KIND_TOO_LARGE).message);
			}
			if (status != HOWDY_FFI_STATUS_OK || handle == nullptr) {
				return DecodeFailure(UserModelStatus::kParseError, kDecodeFailedMessage);
			}
			bool                    has_error = true;
			HowdyUserModelErrorKind kind      = HOWDY_USER_MODEL_ERROR_KIND_TRUNCATED;
			if (howdy_user_model_decoded_error(handle.get(), &has_error, &kind) !=
			    HOWDY_FFI_STATUS_OK) {
				return DecodeFailure(UserModelStatus::kParseError, kDecodeFailedMessage);
			}
			if (has_error) {
				const auto failure = DescribeError(kind);
				return DecodeFailure(failure.status, failure.message);
			}
			std::size_t count = 0;
			if (howdy_user_model_decoded_count(handle.get(), &count) != HOWDY_FFI_STATUS_OK ||
			    count == 0) {
				return DecodeFailure(UserModelStatus::kParseError, kDecodeFailedMessage);
			}

			UserModelListResult result{.status = UserModelStatus::kOk};
			result.entries.reserve(count);
			for (std::size_t index = 0; index < count; ++index) {
				auto entry = ReadEntry(handle.get(), index);
				if (!entry.has_value()) {
					return DecodeFailure(UserModelStatus::kParseError, kDecodeFailedMessage);
				}
				result.entries.push_back(std::move(*entry));
			}
			std::uint32_t next_id = 0;
			if (howdy_user_model_decoded_next_id(handle.get(), &next_id) != HOWDY_FFI_STATUS_OK) {
				return DecodeFailure(UserModelStatus::kParseError, kDecodeFailedMessage);
			}
			// The codec caps the next id at kMaxId + 1, so it fits an int.
			result.next_id = static_cast<int>(next_id);
			return result;
		}

		auto EncodeEntries(std::span<const UserModelEntry> entries, int next_id) -> EncodeResult {
			std::vector<std::vector<HowdyUserModelEncoding>> encodings;
			std::vector<HowdyUserModelEntryInput>            inputs;
			encodings.reserve(entries.size());
			inputs.reserve(entries.size());
			// Bound every count on the C++ side before Rust sees it; Rust checks them again.
			if (entries.size() > kMaxModels) {
				const auto failure = DescribeError(HOWDY_USER_MODEL_ERROR_KIND_TOO_MANY_MODELS);
				return EncodeFailure(failure.status, failure.message);
			}
			for (const auto &entry : entries) {
				if (entry.encodings.size() > kMaxEncodingsPerModel) {
					const auto failure =
					    DescribeError(HOWDY_USER_MODEL_ERROR_KIND_TOO_MANY_ENCODINGS);
					return EncodeFailure(failure.status, failure.message);
				}
				if (std::ranges::any_of(entry.encodings, [](const auto &encoding) -> bool {
					    return encoding.size() > kMaxEncodingLength;
				    })) {
					const auto failure = DescribeError(HOWDY_USER_MODEL_ERROR_KIND_ENCODING_LENGTH);
					return EncodeFailure(failure.status, failure.message);
				}
				if (entry.id < 0) {
					const auto failure = DescribeError(HOWDY_USER_MODEL_ERROR_KIND_ID_OUT_OF_RANGE);
					return EncodeFailure(failure.status, failure.message);
				}
				auto &views = encodings.emplace_back();
				views.reserve(entry.encodings.size());
				for (const auto &encoding : entry.encodings) {
					views.push_back({.values = encoding.data(), .len = encoding.size()});
				}
				inputs.push_back({
				    .id             = static_cast<std::uint32_t>(entry.id),
				    .time           = entry.time,
				    .metric         = MetricCode(entry.metric),
				    .label          = View(entry.label),
				    .backend        = View(entry.backend),
				    .model          = View(entry.model),
				    .encodings      = views.data(),
				    .encoding_count = views.size(),
				});
			}

			// After the entries, matching the codec's order: id rules come before the next id.
			if (next_id < 0) {
				const auto failure = DescribeError(HOWDY_USER_MODEL_ERROR_KIND_INVALID_NEXT_ID);
				return EncodeFailure(failure.status, failure.message);
			}

			HowdyUserModelEncoded *raw = nullptr;
			const auto status = howdy_user_model_encode(static_cast<std::uint32_t>(next_id),
			                                            inputs.data(), inputs.size(), &raw);
			const EncodedHandle handle(raw);
			if (status != HOWDY_FFI_STATUS_OK || handle == nullptr) {
				return EncodeFailure(UserModelStatus::kWriteFailed, kEncodeFailedMessage);
			}
			bool                    has_error = true;
			HowdyUserModelErrorKind kind      = HOWDY_USER_MODEL_ERROR_KIND_ENCODE_MISMATCH;
			if (howdy_user_model_encoded_error(handle.get(), &has_error, &kind) !=
			    HOWDY_FFI_STATUS_OK) {
				return EncodeFailure(UserModelStatus::kWriteFailed, kEncodeFailedMessage);
			}
			if (has_error) {
				const auto failure = DescribeError(kind);
				return EncodeFailure(failure.status, failure.message);
			}
			HowdyStr content{};
			if (howdy_user_model_encoded_content(handle.get(), &content) != HOWDY_FFI_STATUS_OK) {
				return EncodeFailure(UserModelStatus::kWriteFailed, kEncodeFailedMessage);
			}
			return EncodeResult{.content = std::string(Text(content))};
		}

	}  // namespace

	// Running out of memory while copying entries is reported like any other codec failure, so
	// storage APIs keep returning a status instead of throwing.
	auto Decode(std::string content) -> UserModelListResult {
		try {
			return DecodeEntries(content);
		} catch (const std::bad_alloc &) {
			return DecodeFailure(UserModelStatus::kParseError, kDecodeFailedMessage);
		}
	}

	auto Encode(std::span<const UserModelEntry> entries, int next_id) -> EncodeResult {
		try {
			return EncodeEntries(entries, next_id);
		} catch (const std::bad_alloc &) {
			return EncodeFailure(UserModelStatus::kWriteFailed, kEncodeFailedMessage);
		}
	}

	auto NextIdAfter(int id) -> int {
		std::uint32_t next_id = 0;
		if (id < 0 || howdy_user_model_next_id_after(static_cast<std::uint32_t>(id), &next_id) !=
		                  HOWDY_FFI_STATUS_OK) {
			return id;  // Not a storable id; encoding rejects the entry.
		}
		// The codec caps the next id at kMaxId + 1, so it fits an int.
		return static_cast<int>(next_id);
	}

	auto CheckModelLabel(std::string_view label) -> LabelProblem {
		HowdyUserModelLabelProblem problem = HOWDY_USER_MODEL_LABEL_PROBLEM_MALFORMED;
		if (howdy_user_model_label_problem(View(label), &problem) != HOWDY_FFI_STATUS_OK) {
			return LabelProblem::kMalformed;
		}
		switch (problem) {
			case HOWDY_USER_MODEL_LABEL_PROBLEM_NONE:
				return LabelProblem::kNone;
			case HOWDY_USER_MODEL_LABEL_PROBLEM_EMPTY:
				return LabelProblem::kEmpty;
			case HOWDY_USER_MODEL_LABEL_PROBLEM_TOO_LONG:
				return LabelProblem::kTooLong;
			case HOWDY_USER_MODEL_LABEL_PROBLEM_HIDDEN:
				return LabelProblem::kHidden;
			case HOWDY_USER_MODEL_LABEL_PROBLEM_MALFORMED:
				break;
		}
		return LabelProblem::kMalformed;
	}

	auto IsValidModelLabel(std::string_view label) -> bool {
		return CheckModelLabel(label) == LabelProblem::kNone;
	}

	auto ValidateNewLabel(std::string_view label) -> EncodeResult {
		if (IsValidModelLabel(label)) {
			return EncodeResult{};
		}
		const auto failure = DescribeError(HOWDY_USER_MODEL_ERROR_KIND_INVALID_LABEL);
		return EncodeFailure(failure.status, failure.message);
	}

	auto DisplayModelLabel(std::string_view label) -> std::string {
		std::array<char, HOWDY_USER_MODEL_MAX_DISPLAY_LABEL_BYTES> shown{};
		std::size_t                                                length = 0;
		if (howdy_user_model_display_label(View(label), shown.data(), shown.size(), &length) !=
		    HOWDY_FFI_STATUS_OK) {
			return "<unprintable label>";
		}
		return {shown.data(), length};
	}

	auto ModelLabelPrefixLength(std::string_view text) -> std::optional<std::size_t> {
		std::size_t length = 0;
		if (howdy_user_model_label_prefix_len(View(text), &length) != HOWDY_FFI_STATUS_OK) {
			return std::nullopt;
		}
		return length;
	}

}  // namespace howdy::native::user_model_codec
