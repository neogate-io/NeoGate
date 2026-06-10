# Backend Tests

## OpenAI image smoke tests

The image smoke tests live in `test_openai_image.py` and read defaults from
`../.env` (`backend/.env`). Environment variables override values from the env
file.

Required:

```bash
NEOGATE_API_KEY=your_neogate_api_key
```

Optional:

```bash
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1
NEOGATE_IMAGE_SIZE=1536x1024
```

The image model is fixed to `gpt-image-2`, and generated images are saved under
`output/`.

Run all image smoke tests:

```bash
python -m unittest test_openai_image
```

Run one test:

```bash
python -m unittest test_openai_image.test_images_generation_json
python -m unittest test_openai_image.test_images_edit_multipart
python -m unittest test_openai_image.test_images_generation_stream
python -m unittest test_openai_image.test_images_edit_json_stream
python -m unittest test_openai_image.test_images_variation
python -m unittest test_openai_image.test_responses_image_generation_background
```

The `output/` directory is ignored by git except for its `.gitignore` file.
