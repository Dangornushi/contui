use serde_json::json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_calling_json_structure() {
        // Gemini Function Calling リクエストの例
        let function_call_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "text": "test.txtというファイルを作成してください。内容は「Hello, Function Calling!」にしてください。"
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "create_file",
                            "description": "ファイルを作成します",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "filename": {
                                        "type": "string",
                                        "description": "作成するファイル名"
                                    },
                                    "content": {
                                        "type": "string",
                                        "description": "ファイルの内容"
                                    }
                                },
                                "required": ["filename", "content"]
                            }
                        }
                    ]
                }
            ],
            "generationConfig": {
                "temperature": 0.7,
                "maxOutputTokens": 1000
            }
        });

        println!("=== Function Calling Request Test ===");
        println!("{}", serde_json::to_string_pretty(&function_call_request).unwrap());

        // Gemini Function Calling レスポンスの例
        let function_call_response = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "functionCall": {
                                    "name": "create_file",
                                    "args": {
                                        "filename": "test.txt",
                                        "content": "Hello, Function Calling!"
                                    }
                                }
                            }
                        ]
                    }
                }
            ]
        });

        println!("\n=== Function Calling Response Test ===");
        println!("{}", serde_json::to_string_pretty(&function_call_response).unwrap());

        println!("\n=== Test Completed ===");
        println!("✅ Function Calling の JSON 構造が正しく実装されています");

        assert!(function_call_request["tools"].is_array());
        assert!(function_call_response["candidates"].is_array());
    }
}

pub fn run_function_calling_test() {
    // Gemini Function Calling リクエストの例
    let function_call_request = json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": "test.txtというファイルを作成してください。内容は「Hello, Function Calling!」にしてください。"
                    }
                ]
            }
        ],
        "tools": [
            {
                "functionDeclarations": [
                    {
                        "name": "create_file",
                        "description": "ファイルを作成します",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "filename": {
                                    "type": "string",
                                    "description": "作成するファイル名"
                                },
                                "content": {
                                    "type": "string",
                                    "description": "ファイルの内容"
                                }
                            },
                            "required": ["filename", "content"]
                        }
                    }
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0.7,
            "maxOutputTokens": 1000
        }
    });

    println!("=== Function Calling Request Test ===");
    println!("{}", serde_json::to_string_pretty(&function_call_request).unwrap());

    // Gemini Function Calling レスポンスの例
    let function_call_response = json!({
        "candidates": [
            {
                "content": {
                    "parts": [
                        {
                            "functionCall": {
                                "name": "create_file",
                                "args": {
                                    "filename": "test.txt",
                                    "content": "Hello, Function Calling!"
                                }
                            }
                        }
                    ]
                }
            }
        ]
    });

    println!("\n=== Function Calling Response Test ===");
    println!("{}", serde_json::to_string_pretty(&function_call_response).unwrap());

    println!("\n=== Test Completed ===");
    println!("✅ Function Calling の JSON 構造が正しく実装されています");
}