# Gemini API: generateContent Request Specification

This document describes the JSON request body for the `generateContent` method of the Gemini API.

## Request Body Structure

The request body is a JSON object with the following primary fields:

```json
{
  "contents": [
    {
      "role": "user",
      "parts": [
        {
          "text": "Your prompt here."
        }
      ]
    }
  ],
  "tools": [],
  "tool_config": {},
  "safety_settings": [],
  "generation_config": {}
}
```

### `contents` (required)

An array of `Content` objects, representing the conversation history. Each `Content` object contains a `role` and an array of `parts`.

#### `Content` Object Structure

*   **`role`** (string): The role of the author of this content.
    *   Allowed values: `"user"`, `"model"`
*   **`parts`** (array of `Part` objects): An array of `Part` objects, which can be of various types.

#### `Part` Object Types

*   **Text Part**: For text-based content.
    ```json
    {
      "text": "Your text content here."
    }
    ```
*   **Inline Data Part**: For inline data like images.
    ```json
    {
      "inlineData": {
        "mimeType": "image/jpeg",
        "data": "base64_encoded_image_data"
      }
    }
    ```
    *   `mimeType` (string): The MIME type of the data (e.g., `image/png`, `image/jpeg`).
    *   `data` (string): The base64 encoded string of the data.

### `tools` (optional)

An array of `Tool` objects. These define functions or code execution capabilities that the model can use.

### `tool_config` (optional)

Configuration for any `Tool` specified in the request.

### `safety_settings` (optional)

An array of `SafetySetting` objects. These settings allow you to define thresholds for blocking unsafe content across different safety categories.

### `generation_config` (optional)

An object that configures the generation parameters for the model's response.

#### `GenerationConfig` Object Fields

*   **`temperature`** (number): Controls the randomness of the output. Higher values mean more random. (e.g., `0.0` to `1.0`)
*   **`max_output_tokens`** (integer): The maximum number of tokens to generate in the response.
*   **`top_p`** (number): The maximum cumulative probability of tokens to consider.
*   **`top_k`** (integer): The maximum number of tokens to consider.
*   **`stop_sequences`** (array of strings): A list of sequences that will stop the generation.
